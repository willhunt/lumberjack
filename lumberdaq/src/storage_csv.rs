use crate::channel::ChannelInfo;
use crate::daq::{ Daq, DaqInfo };
use crate::device::DeviceInfo;
use crate::Result;
use std::ffi::OsStr;
use std::io::{Write, ErrorKind};
use std::fs::File;
use chrono::{ DateTime, Utc };
use serde_json::to_string_pretty;
use serde::{Deserialize, Serialize};

/// The strategy for storing data as it is being recorded is to use a csv file
/// Through testing this is much faster than saving a SQLite data base to disk
/// and HDF5 is also not suited for regular write operations. The data can 
/// later be converted to HDF5 or another format if required.
/// 
/// To maintain a typical format for the csv file a json will be used to store
/// any additional information about the test or setup.

#[derive(Serialize, Deserialize)]
struct DeviceHeader {
    info: DeviceInfo,
    channels: Vec<ChannelInfo>,
}
#[derive(Serialize, Deserialize)]
struct DaqHeader {
    info: DaqInfo,
    devices: Vec<DeviceHeader>,
}

pub fn check_file_extension(path: &std::path::PathBuf, extension: &OsStr) -> Result<()> {
    if path.extension() != Some(extension) {
        let error_msg = format!("Incorrect path extension. The extension must be {:?} but {:?} was provided.",
            &extension,
            &path.extension()    
        );
        return Err(Box::new(std::io::Error::new(ErrorKind::InvalidData, error_msg)));
    }
    Ok(())
}

pub fn create_json_file(path: &std::path::PathBuf, daq: &Daq) -> Result<()> {
    check_file_extension(path, OsStr::new("json"))?;
    let header = DaqHeader{
        info: daq.info.clone(),
        devices: daq.devices.iter().map(|device|
            DeviceHeader {
                info: device.info.clone(),
                channels: device.channels.iter().map(|channel| channel.info.clone()).collect(),
            }
        ).collect(),
    };
    // serde_json::to_writer(writer, value);

    let mut file = File::create(path)?;
    // file.write_all(header_json.to_string().as_bytes())?;
    file.write_all(to_string_pretty(&header)?.as_bytes())?;
    return Ok(());
}

pub fn create_csv_file(path: &std::path::PathBuf) -> Result<csv::Writer<std::fs::File>> {
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