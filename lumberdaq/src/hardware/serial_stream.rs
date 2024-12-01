use crate::channel::{ Channel, ChannelData, ChannelDataAquisition, ChannelInfo };
use crate::device::{ Device, DeviceDataAquisition, DeviceInfo };
use serialport;
use chrono;

pub fn create_device(name: String, port: serialport::SerialPortInfo) -> Device {
    Device {
        info: DeviceInfo {
            name: name,
            description: "Stream of serial data to read.".to_string(),
        },
        channels: Vec::new(),
        config: Box::new(NiUsb6001Config {
            port: port,
        }),
    }
}

pub struct SerialStreamConfig {
    pub port: serialport::SerialPortInfo,
}
impl DeviceDataAquisition for SerialStreamConfig {
    fn connect(&self) {
        println!("Connected to serial stream device.");
    }
    // fn read(&mut self) {
    //     for channel in &mut self.device.channels {
    //         match channel.read() {
    //             Err(e) => println!("{e}"),
    //             Ok(()) => (),
    //         }
    //     }
    // }
}