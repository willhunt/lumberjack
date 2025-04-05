use crate::{daq, Result};
use crate::datapoint::DataPoint;
use crate::channel::{ Channel, ChannelData, ChannelDataAquisition, ChannelInfo };
use crate::device::{ Device, DeviceDataAquisition, DeviceInfo };
use serde::{Deserialize, Serialize};
// use serialport;
use chrono;
use std::any::Any;
#[allow(dead_code)]
use daqmx::tasks::{AnalogInput, InputTask, Task};
#[allow(dead_code)]
use daqmx::channels::VoltageChannelBuilder;
#[allow(dead_code)]
use daqmx::types::Timeout;

pub fn create_device(name: String, port: String) -> Result<Device> {
    let mut task = Task::new(&name)?;
    Ok(Device {
        info: DeviceInfo {
            name: name,
            description: "National instruments USB-6001 data aquisition device.".to_string(),
        },
        channels: Vec::new(),
        config: Box::new(NiUsb6001Config {
            // port: port,
            task: task,
        }),
    })
}

#[derive(Serialize, Deserialize)]
pub struct NiUsb6001Config {
    // pub port: String,
    task: Task<AnalogInput>,
}
impl DeviceDataAquisition for NiUsb6001Config {
    fn connect(&mut self) {
        // self.task.start();  // Optional
        println!("Connected to NI USB-6001.");
    }
    fn read(&mut self, channels: &mut Vec<Channel>) -> Result<()> {
        let mut buffer: Vec<f64> = vec![0.0; channels.len()];
        let timestamp = chrono::Utc::now();
        self.task.read(Timeout::Seconds(1.0), daqmx::types::DataFillMode::GroupByChannel, Some(1), &mut buffer)?;
        for (channel, reading) in channels.iter_mut().zip(buffer.iter()) {
            let mut datapoints = vec![DataPoint {
                datetime: timestamp,
                value: *reading,
            }];
            channel.data.add_datapoints(&mut datapoints)?;
        }
        Ok(())
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

pub struct AnalogChannelConfig {
    // port: String,
    // task: Task<AnalogInput>,
}
impl ChannelDataAquisition for AnalogChannelConfig { 
    fn read(&mut self) -> Result<Vec<DataPoint>> {
        // Ok(vec![DataPoint {
        //     datetime: chrono::Utc::now(),
        //     value: self.task.read_scalar(Timeout::Seconds(1.0))?,
        // }])
        Err("Channels for this device must be read all together by the device read method.".into())
    }
}
pub fn add_channel_analog(device: &mut Device, name: String, port: String) -> Result<()> {
    // let mut task = Task::new(&name)?;
    let config_niusb6001 = match device.config.as_any_mut().downcast_mut::<NiUsb6001Config>() {
        Some(config) => config,
        None => return Err("The device must have a config of type NiUsb6001Config".into()),
    };
    let voltage_channel = VoltageChannelBuilder::new(port.to_string())?;
    config_niusb6001.task.create_channel(voltage_channel)?;

    let channel = Channel {
        info: ChannelInfo::new(name, "V".to_string(), "Analog input channel".to_string()),
        data: ChannelData::new(),
        config: Box::new(AnalogChannelConfig {
            // port: port,
            // task: task,
        }),
    };
    device.add_channel(channel)?;
    Ok(())
}
