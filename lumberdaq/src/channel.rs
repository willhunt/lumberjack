use crate::Result;
use crate::datapoint::DataPoint;
use crate::storage_csv::write_csv_record;
use chrono;
use serde::{Deserialize, Serialize};

pub trait ChannelDataAquisition {
    fn read(&mut self) -> Result<Vec<DataPoint>>;
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ChannelInfo {
    pub id: String,
    pub name: String,
    pub unit: String,
    pub description: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Channel {
    pub info: ChannelInfo,
    pub datapoints: Vec<DataPoint>,
    pub datapoint_last: Option<DataPoint>,
}

impl Channel {
    // pub fn read(&mut self) -> Result<()> {
    //     let mut datapoints = self.config.read()?;
    //     self.data.add_datapoints(&mut datapoints)?;
    //     Ok(())
    // }
    pub fn new(id: String, name: String, unit: String, description: String) -> Channel {
        Channel {
            info: ChannelInfo {
                id: id,
                name: name,
                unit: unit,
                description: description,
            },
            datapoints: vec![],
            datapoint_last: None,
        }
    }

    pub fn add_datapoints(&mut self, datapoints: &mut Vec<DataPoint>) -> Result<()> {
        self.datapoints.append(datapoints);
        self.datapoint_last = self.datapoints.last().copied();
        return Ok(());
    } 

    pub fn latest_as_string(&self) -> String {
        match self.datapoint_last {
            Some(data) => format!("{}: {}, {} {}", self.info.name, data.datetime, data.value, self.info.unit),
            None => format!("{}: No data", self.info.name)
        }
    }

    pub fn write(&mut self, wtr: &mut csv::Writer<std::fs::File>, device_name: &str) -> Result<()> {
        for datapoint in self.datapoints.iter() {
            write_csv_record(
                wtr,
                device_name,
                &self.info.name,
                datapoint.datetime,
                datapoint.value
            )?;
        }
        self.datapoints = vec![];
        Ok(())
    }

    pub fn datapoints_as_vectors(&self) -> Result<(Vec<chrono::DateTime<chrono::Utc>>, Vec<f64>)> {
        let mut datetimes: Vec<chrono::DateTime<chrono::Utc>> = Vec::new();
        let mut values: Vec<f64> = Vec::new();
        for datapoint in self.datapoints.iter() {
            datetimes.push(datapoint.datetime);
            values.push(datapoint.value);
        }
        return Ok((datetimes, values));
    }
}