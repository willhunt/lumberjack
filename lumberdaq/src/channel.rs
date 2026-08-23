use crate::Result;
use crate::datapoint::DataPoint;
use crate::storage::Batch;
use chrono;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
/// What a channel is called and what its numbers mean.
///
/// Note there is no id here. Which input a channel reads is recorded by the
/// hardware config, in the one place that binding is defined; a second
/// identifier alongside the name only invited confusion with the row ids in a
/// results database.
pub struct ChannelInfo {
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
    pub fn new(name: String, unit: String, description: String) -> Channel {
        Channel::from_info(ChannelInfo {
            name: name,
            unit: unit,
            description: description,
        })
    }

    /// Start an empty channel from its description in a config.
    pub fn from_info(info: ChannelInfo) -> Channel {
        Channel {
            info: info,
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

    /// Hand off everything acquired so far, leaving the channel's buffer empty.
    ///
    /// `mem::take` swaps in an empty Vec and returns the old one, so the
    /// datapoints move into the batch rather than being copied.
    pub fn drain_batch(&mut self, device_name: &str) -> Batch {
        Batch {
            device: device_name.to_string(),
            channel: self.info.name.clone(),
            datapoints: std::mem::take(&mut self.datapoints),
        }
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