use crate::Result;
use crate::datapoint::DataPoint;
use crate::storage_csv::write_csv_record;
use serde::{Deserialize, Serialize};

// pub enum WriteStatus {
//     // YesToWrite,
//     NoToWrite,
//     WriteComplete,
// }

pub trait ChannelDataAquisition {
    fn read(&self) -> Vec<DataPoint>;
}

#[derive(Serialize, Deserialize)]
pub struct ChannelInfo {
    pub name: String,
    pub unit: String,
    pub description: String,
}

impl ChannelInfo {
    pub fn new(name: String, unit: String, description: String) -> ChannelInfo {
        ChannelInfo {
            name: name,
            unit: unit,
            description: description,
        }
    }
}

pub struct ChannelData {
    datapoints: Vec<DataPoint>,
    pub datapoint_last: Option<DataPoint>,
}

impl ChannelData {
    pub fn new() -> ChannelData {
        ChannelData {
            datapoints: vec![],
            datapoint_last: None,
        }
    }

    pub fn add_datapoints(&mut self, datapoints: &mut Vec<DataPoint>) -> Result<()> {
        self.datapoints.append(datapoints);
        self.datapoint_last = self.datapoints.last().copied();
        return Ok(());
    }   
}

// Could try parent Channel type that then holds data and config. The config would then have traits read. This might make it less inheritancy
pub struct Channel {
    pub info: ChannelInfo,
    pub data: ChannelData,
    pub config: Box<dyn ChannelDataAquisition>,
}

impl Channel {
    pub fn read(&mut self) -> Result<()> {
        let mut datapoints = self.config.read();
        self.data.add_datapoints(&mut datapoints)?;
        Ok(())
    }

    pub fn latest_as_string(&self) -> String {
        match self.data.datapoint_last {
            Some(data) => format!("{}: {}, {}{}", self.info.name, data.datetime, data.value, self.info.unit),
            None => format!("{}: No data", self.info.name)
        }
    }

    pub fn write(&mut self, wtr: &mut csv::Writer<std::fs::File>, device_name: &str) -> Result<()> {
        for datapoint in self.data.datapoints.iter() {
            write_csv_record(
                wtr,
                device_name,
                &self.info.name,
                datapoint.datetime,
                datapoint.value
            )?;
        }
        self.data.datapoints = vec![];
        Ok(())
    }
}