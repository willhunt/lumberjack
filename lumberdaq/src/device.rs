use crate::Result;
use crate::channel::Channel;
use crate::hardware::{ Hardware, HardwareDataAquisition };
use serde::{Deserialize, Serialize};

pub trait DeviceInterface {
    fn connect(&mut self);
    // fn read(&mut self) -> Result<()>;
}

#[derive(Serialize, Deserialize, Clone)]
pub struct DeviceInfo {
    pub name: String,
    pub description: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Device {
    pub info: DeviceInfo,
    pub channels: Vec<Channel>,
    pub hardware: Hardware,
}

impl Device {
    pub fn new(name: String, description: String, hardware: Hardware) -> Device {
        Device {
            info: DeviceInfo {
                name: name,
                description: description,
            },            
            channels: vec![],
            hardware: hardware,
        }
    }

    pub fn add_channel(&mut self, channel: Channel) -> Result<()> {
        for existing_channel in self.channels.iter() {
            if existing_channel.info.name == channel.info.name {
                return Err("Channel name must be unique".into());
            }
        }
        self.channels.push(channel);
        Ok(())
    }

    pub fn print_latest(&self) {
        println!("Latest reading from device: {}", &self.info.name);
        for channel in self.channels.iter() {
            println!("    {}", channel.latest_as_string());
        }
    }

    pub fn read(&mut self) -> Result<()> {
        let mut input_readings = self.hardware.read()?;
        for (channel, datapoints) in self.channels.iter_mut().zip(input_readings.iter_mut()) {
            channel.add_datapoints(datapoints);
        }
        Ok(())
    }

    pub fn write(&mut self, wtr: &mut csv::Writer<std::fs::File>) -> Result<()>{
        for channel in &mut self.channels {
            channel.write(wtr, &self.info.name)?;
        }
        Ok(())
    }
    
    // pub fn add_to_hdf(&self, file: )

}

impl DeviceInterface for Device {
    fn connect(&mut self) {
        self.hardware.connect();
    }
}