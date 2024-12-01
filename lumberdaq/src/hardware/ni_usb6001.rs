use crate::{daq, Result};
use crate::datapoint::DataPoint;
use crate::channel::{ Channel, ChannelData, ChannelDataAquisition, ChannelInfo };
use crate::device::{ Device, DeviceDataAquisition, DeviceInfo };
use serialport;
use chrono;
use daqmx::tasks::{AnalogInput, InputTask, Task};
use daqmx::channels::VoltageChannelBuilder;
use daqmx::types::Timeout;

pub fn create_device(name: String, port: String) -> Device {
    Device {
        info: DeviceInfo {
            name: name,
            description: "National instruments USB-6001 data aquisition device.".to_string(),
        },
        channels: Vec::new(),
        config: Box::new(NiUsb6001Config {
            port: port,
        }),
    }
}
pub struct NiUsb6001Config {
    pub port: String,
}
impl DeviceDataAquisition for NiUsb6001Config {
    fn connect(&self) {
        println!("Connected to NI USB-6001.");
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

pub struct AnalogChannelConfig {
    port: String,
    task: Task<AnalogInput>,
}
impl ChannelDataAquisition for AnalogChannelConfig {
    fn read(&mut self) -> Result<Vec<DataPoint>> {
        self.task.read(timeout, fill_mode, samples_per_channel, buffer)
        Ok(vec![DataPoint {
            datetime: chrono::Utc::now(),
            value: self.task.read_scalar(Timeout::Seconds(1.0))?,
        }])
    }
}
pub fn add_channel_analog(device: &mut Device, name: String, port: String) -> Result<()> {
    let mut task = Task::new("scalar")?;
    let voltage_channel = VoltageChannelBuilder::new("NIUSB-6001/ai0")?;
    task.create_channel(voltage_channel)?;
    task.read_scalar(Timeout::Seconds(1.0))?;

    let channel = Channel {
        info: ChannelInfo::new(name, "V".to_string(), "Analog input channel".to_string()),
        data: ChannelData::new(),
        config: Box::new(AnalogChannelConfig {
            port: port,
            task: task,
        }),
    };
    device.add_channel(channel)?;
    Ok(())
}
