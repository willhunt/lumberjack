use crate::channel::Channel;
use crate::daq::Daq;
use crate::datapoint::DataPoint;
use crate::device::{ Device, ConnectionStatus };
use crate::hardware::Hardware;
use crate::config::DaqConfig;
use crate::storage::{ Batch, DaqHeader, DataSink };
use crate::Result;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::{ ErrorKind, Read, Write };
use std::fs::File;
use std::path::{ Path, PathBuf };
use chrono::{ DateTime, Utc };
use serde_json::to_string_pretty;

/// The strategy for storing data as it is being recorded is to use a csv file
/// Through testing this is much faster than saving a SQLite data base to disk
/// and HDF5 is also not suited for regular write operations.
/// 
/// To maintain a typical format for the csv file a json will be used to store
/// any additional information about the test or setup.


pub fn check_file_extension(path: &std::path::PathBuf, extension: &OsStr) -> Result<()> {
    if path.extension() != Some(extension) {
        let error_msg = format!("Incorrect path extension. The extension must be {:?} but {:?} was provided.",
            &extension,
            &path.extension()    
        );
        return Err(std::io::Error::new(ErrorKind::InvalidData, error_msg).into());
    }
    Ok(())
}

pub fn write_json_file(path: &std::path::PathBuf, daq: &Daq) -> Result<()> {
    write_json_header(path, &DaqHeader::from_config(&daq.config()))
}

pub fn write_json_header(path: &Path, header: &DaqHeader) -> Result<()> {
    check_file_extension(&path.to_path_buf(), OsStr::new("json"))?;
    let mut file = File::create(path)?;
    file.write_all(to_string_pretty(header)?.as_bytes())?;
    return Ok(());
}

pub fn read_json_file(path: &std::path::PathBuf) -> Result<DaqHeader> {
    let mut file = File::open(path)?;
    let mut data = String::new();
    file.read_to_string(&mut data)?;

    let header: DaqHeader = serde_json::from_str(&data)?;
    return Ok(header);
}

pub fn write_csv_file(path: &std::path::PathBuf) -> Result<csv::Writer<std::fs::File>> {
    check_file_extension(path, OsStr::new("csv"))?;
    let mut wtr = csv::Writer::from_path(path)?;
    wtr.write_record(&["Device", "Channel", "Timestamp", "Value"])?;
    return Ok(wtr);
}

pub fn write_csv_record(wtr: &mut csv::Writer<std::fs::File>, device_name: &str, channel_name: &str, timestamp: DateTime<Utc>, value: f64) -> Result<()> {
    wtr.write_record(&[device_name, channel_name, &timestamp.to_string(), &value.to_string()])?;
    wtr.flush()?;
    return Ok(());
}

/// Streams acquired data to a csv file, with a json sidecar holding the header.
///
/// The csv is long format: one row per datapoint, repeating the device and
/// channel name on every row. That is verbose on disk, but it is append-only
/// and line oriented, so a run that dies halfway still leaves a file that reads.
pub struct CsvSink {
    writer: csv::Writer<File>,
    json_path: PathBuf,
}

impl CsvSink {
    pub fn new(csv_path: &Path, json_path: &Path) -> Result<CsvSink> {
        check_file_extension(&csv_path.to_path_buf(), OsStr::new("csv"))?;
        check_file_extension(&json_path.to_path_buf(), OsStr::new("json"))?;
        Ok(CsvSink {
            writer: csv::Writer::from_path(csv_path)?,
            json_path: json_path.to_path_buf(),
        })
    }
}

impl DataSink for CsvSink {
    fn init(&mut self, config: &DaqConfig) -> Result<()> {
        // The csv only needs the labels, so it flattens the setup and writes
        // that. The hardware details are dropped, which is why a csv run cannot
        // later say which port or frame index produced it.
        write_json_header(&self.json_path, &DaqHeader::from_config(config))?;
        self.writer.write_record(&["Device", "Channel", "Timestamp", "Value"])?;
        self.flush()
    }

    fn write_batch(&mut self, batch: &Batch) -> Result<()> {
        // No flush per row: csv::Writer buffers, and flushing every datapoint
        // costs a syscall per sample. The caller decides how often to flush,
        // which is what bounds how much data a crash can lose.
        for datapoint in batch.datapoints.iter() {
            self.writer.write_record(&[
                &batch.device,
                &batch.channel,
                &datapoint.datetime.to_string(),
                &datapoint.value.to_string(),
            ])?;
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        self.writer.flush()?;
        Ok(())
    }
}

pub fn read_csv_file(path: &std::path::PathBuf) -> Result<HashMap<String, HashMap<String, Vec<DataPoint>>>> {
    // let read_data: Vec<ChannelReadData> = vec![];
    let mut device_map = HashMap::new();

    // let file = File::open(path)?;
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_path(path)?;
    for result  in rdr.records() {
        let record = result?;
        if record.len() < 4 {
            return Err("csv record not of length 4".into());
        }
        let device_name = record[0].to_string();
        let channel_name = record[1].to_string();
        let timestamp: DateTime<Utc> = record[2].parse()?;
        let value: f64 = record[3].parse()?;

        let datapoint = DataPoint{datetime: timestamp, value: value};

        let channel_map = device_map.entry(device_name).or_insert(HashMap::new());
        let datapoints = channel_map.entry(channel_name).or_insert(Vec::new());
        datapoints.push(datapoint);
    }
    Ok(device_map)
}

pub fn read_results(csv_path: &std::path::PathBuf, json_path: &std::path::PathBuf) -> Result<Daq> {
    let header = read_json_file(json_path)?;
    let device_map = read_csv_file(csv_path)?;

    let mut devices: Vec<Device> = Vec::new();
    for device_header in header.devices.iter() {
        let channel_map = match device_map.get(&device_header.info.name) {
            Some(map) => map,
            _ => return Err(format!("Device '{}' missing from csv read", &device_header.info.name).into()),
        };
        let mut channels: Vec<Channel> = Vec::new();
        for channel_info in device_header.channels.iter() {
            let datapoints  = match channel_map.get(&channel_info.name) {
                Some(data) => data,
                None => return Err(format!("Channel '{}' missing from csv read", &channel_info.name).into()),
            };
            let channel = Channel {
                info: channel_info.clone(),
                datapoints: datapoints.clone(),
                datapoint_last: None,
            };
            channels.push(channel);
        }
        let device = Device {
            info: device_header.info.clone(),
            read_interval: std::time::Duration::from_millis(
                crate::config::default_read_interval_ms(),
            ),
            channels: channels,
            hardware: Hardware::None,
            connection: ConnectionStatus::default(),
        };
        devices.push(device);

    }
    let daq = Daq {
        info: header.info,
        storage: crate::config::StorageFormat::Csv,
        calculated: None,
        devices: devices,
        sink: None,
    };
    return Ok(daq);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;
    use std::path::PathBuf;

    #[test]
    fn test_correct_extension() {
        let path = PathBuf::from("test_file.txt");
        let extension = OsStr::new("txt");
        let result = check_file_extension(&path, extension);
        assert!(result.is_ok());
    }

    #[test]
    fn test_wrong_extension() {
        let path = PathBuf::from("test_file.pdf");
        let extension = OsStr::new("txt");
        let result = check_file_extension(&path, extension);
        assert!(result.is_err());
        if let Err(e) = result {
            let error_message = e.to_string();
            assert!(error_message.contains("Incorrect path extension"));
            assert!(error_message.contains("\"txt\""));
            assert!(error_message.contains("\"pdf\""));
        }
    }

    #[test]
    fn test_no_extension() {
        let path = PathBuf::from("test_file");
        let extension = OsStr::new("txt");
        let result = check_file_extension(&path, extension);
        assert!(result.is_err());
    }

    #[test]
    fn test_case_sensitivity() {
        let path = PathBuf::from("test_file.TXT");
        let extension = OsStr::new("txt");
        let result = check_file_extension(&path, extension);
        // This will fail because extensions are case-sensitive
        assert!(result.is_err());
    }

    #[test]
    fn test_with_multiple_dots() {
        let path = PathBuf::from("test.file.txt");
        let extension = OsStr::new("txt");
        let result = check_file_extension(&path, extension);
        assert!(result.is_ok());
    }
}