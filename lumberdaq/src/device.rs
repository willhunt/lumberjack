use crate::channel::ChannelData;
use serialport;

pub trait DataAquisition {
    fn read(&mut self);
    fn check_and_write(&mut self);
    fn print_latest(&self);
}

#[allow(dead_code)]
pub enum DeviceType {
    Usb {
        baudrate: i32,
    },
    Mock,
}

pub struct Device {
    pub name: String,
    pub port: serialport::SerialPortInfo,
    pub channels: Vec<ChannelData>,
    pub device_type: DeviceType,
}

impl Device {
    pub fn print_latest(&self) {
        println!("Latest reading from device: {}", &self.name);
        for channel in &self.channels {
            println!("    {}", channel.latest_as_string());
        }
    }

    pub fn check_and_write(&mut self) {
        for channel in &mut self.channels {
            channel.check_and_write();
        }
    }
}