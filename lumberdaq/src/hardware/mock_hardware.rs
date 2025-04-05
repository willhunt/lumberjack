use crate::Result;
use crate::datapoint::DataPoint;
use crate::channel::{ Channel, ChannelData, ChannelDataAquisition, ChannelInfo };
use crate::device::{ Device, DeviceDataAquisition, DeviceInfo };
use serde::{Deserialize, Serialize};
// use serialport;
use chrono;
use rand::random;
use std::any::Any;

pub fn create_device(name: String) -> Device {
    Device {
        info: DeviceInfo {
            name: name,
            description: "This is a mock device that uses no hardware. It is used for testing and development purposes.".to_string(),
        },
        channels: Vec::new(),
        config: Box::new(MockConfig{
            // port: serialport::SerialPortInfo {
            //     port_name: "COMx".to_string(),
            //     port_type: serialport::SerialPortType::Unknown
            // },
        }),
    }
}

#[derive(Serialize, Deserialize)]
pub struct MockConfig {

}
impl DeviceDataAquisition for MockConfig {
    fn connect(&mut self) {
        println!("Connected to mock device.");
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

pub struct RandomChannelConfig {
}
impl ChannelDataAquisition for RandomChannelConfig {
    fn read(&mut self) -> Result<Vec<DataPoint>> {
        Ok(vec![DataPoint {
            datetime: chrono::Utc::now(),
            value: random(),
        }])
    }
}
pub fn add_channel_random(device: &mut Device, name: String) -> Result<()> {
    let channel = Channel {
        info: ChannelInfo::new(name, "kW".to_string(), "Random number generator".to_string()),
        data: ChannelData::new(),
        config: Box::new(RandomChannelConfig{}),
    };
    device.add_channel(channel)?;
    Ok(())
}

pub struct ConstantChannelConfig {
    #[allow(dead_code)]
    port: String,
}
impl ChannelDataAquisition for ConstantChannelConfig {
    fn read(&mut self) -> Result<Vec<DataPoint>> {
        Ok(vec![DataPoint {
            datetime: chrono::Utc::now(),
            value: 10.0,
        }])
    }
}
pub fn add_channel_constant(device: &mut Device, name: String) -> Result<()>{
    let channel = Channel {
        info: ChannelInfo::new(name, "degC".to_string(), "Constant number".to_string()),
        data: ChannelData::new(),
        config: Box::new(ConstantChannelConfig{port: "a0".to_string()}),
    };
    device.add_channel(channel)?;
    Ok(())
}


pub struct MockHardwareDevice {
    
}