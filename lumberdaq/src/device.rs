use crate::Result;
use crate::channel::Channel;
use serde::{Deserialize, Serialize};

pub trait DeviceDataAquisition {
    fn connect(&self);
    fn read(&self, channels: &mut Vec<Channel>) {
        for channel in channels.iter_mut() {
            match channel.read() {
                Err(e) => println!("{e}"),
                Ok(()) => (),
            }
        }
    }
}

// #[allow(dead_code)]
// pub enum DeviceType {
//     Usb {
//         baudrate: i32,
//     },
//     Mock,
// }

pub struct Device {
    pub name: String,
    pub description: String,
    pub channels: Vec<Channel>,
    // pub device_type: DeviceType,
    pub device_config: Box<dyn DeviceDataAquisition>,
}

impl Device {
    pub fn add_channel(&mut self, channel: Channel) -> Result<()> {
        for existing_channel in self.channels.iter() {
            if existing_channel.channel_info.name == channel.channel_info.name {
                return Err("Channel name must be unique".into());
            }
        }
        self.channels.push(channel);
        Ok(())
    }

    pub fn print_latest(&self) {
        println!("Latest reading from device: {}", &self.name);
        for channel in &self.channels {
            println!("    {}", channel.latest_as_string());
        }
    }

    pub fn write(&mut self, wtr: &mut csv::Writer<std::fs::File>) -> Result<()>{
        for channel in &mut self.channels {
            channel.write(wtr, &self.name)?;
        }
        Ok(())
    }
    
    // pub fn add_to_hdf(&self, file: )

    // Moved this to trait default. Leaving for now, if other strategy works, delete.
    // pub fn read(&mut self) {
    //     for channel in &mut self.channels {
    //         match channel.read() {
    //             Err(e) => println!("{e}"),
    //             Ok(()) => (),
    //         }
    //     }
    // }
}