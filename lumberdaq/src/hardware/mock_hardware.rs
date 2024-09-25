use crate::datapoint::DataPoint;
use crate::channel::{ Channel, ChannelData, ChannelDataAquisition };
use crate::device::{ Device, DeviceDataAquisition };
use serialport;
use chrono;
use rand::random;

pub fn create_device(name: String) -> Device {
    Device {
        name: name,
        description: "This is a mock device that uses no hardware. It is used for testing and development purposes.".to_string(),
        channels: Vec::new(),
        device_config: Box::new(MockConfig{
            port: serialport::SerialPortInfo {
                port_name: "COMx".to_string(),
                port_type: serialport::SerialPortType::Unknown
            },
        }),
    }
}
pub struct MockConfig {
    pub port: serialport::SerialPortInfo,
}
impl DeviceDataAquisition for MockConfig {
    fn connect(&self) {
        println!("Connected to mock device.");
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

pub struct RandomChannelConfig {
}
impl ChannelDataAquisition for RandomChannelConfig {
    fn read(&self) -> DataPoint {
        DataPoint {
            datetime: chrono::Utc::now(),
            value: random(),
        }
    }
}
pub fn add_channel_random(device: &mut Device, name: String) {
    let channel = Channel {
        channel_data: ChannelData::new(name, "kW".to_string()),
        channel_config: Box::new(RandomChannelConfig{}),
    };
    device.add_channel(channel);
}

pub struct ConstantChannelConfig {
    #[allow(dead_code)]
    port: String,
}
impl ChannelDataAquisition for ConstantChannelConfig {
    fn read(&self) -> DataPoint {
        DataPoint {
            datetime: chrono::Utc::now(),
            value: 10.0,
        }
    }
}
pub fn add_channel_constant(device: &mut Device, name: String) {
    let channel = Channel {
        channel_data: ChannelData::new(name, "degC".to_string()),
        channel_config: Box::new(ConstantChannelConfig{port: "a0".to_string()}),
    };
    device.add_channel(channel);
}
