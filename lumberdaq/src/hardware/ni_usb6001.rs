use crate::Result;
use crate::datapoint::DataPoint;
use crate::channel::{ Channel, ChannelDataAquisition };
use crate::device::{ Device, DeviceInterface };
use crate::hardware::{HardwareDataAquisition, Hardware };
use serde::{ Deserialize, Serialize };
use chrono;
use daqmx::tasks::{ AnalogInput, Task, InputTask };
use daqmx::channels::VoltageChannelBuilder;
use daqmx::types::Timeout;

#[derive(Serialize, Deserialize, Clone)]
pub struct NiUsb6001 {
    decription: String,
    inputs: Vec<NiUsb6001Input>,
    #[serde(skip)]
    task: Option<Task<AnalogInput>>,
}

impl DeviceInterface for NiUsb6001 {
    fn connect(&mut self) -> Result<()> {
        // self.task.start();  // Optional
        println!("Connected to NI USB-6001.");
        Ok(())
    }
}

impl HardwareDataAquisition for NiUsb6001 {
    fn read(&mut self) -> Result<Vec<Vec<DataPoint>>> {
        let mut readings: Vec<Vec<DataPoint>> = vec![];

        let mut buffer: Vec<f64> = vec![0.0; self.inputs.len()];
        let timestamp = chrono::Utc::now();
        match &mut self.task {
            Some(task) => {
                task.read(Timeout::Seconds(1.0), daqmx::types::DataFillMode::GroupByChannel, Some(1), &mut buffer)?;
            },
            None => {
                return Err("No task available to read".into())
            }
        }
        for reading in buffer.iter() {
            let datapoints = vec![DataPoint {
                datetime: timestamp,
                value: *reading,
            }];
            readings.push(datapoints);
        }
        Ok(readings)
    }
}

impl NiUsb6001 {
    pub fn new(name: String) -> Result<NiUsb6001> {
        let mut task = Task::new(&name)?;
        Ok(NiUsb6001 {
            decription: "National instruments USB-6001 data aquisition device.".to_string(),
            inputs: vec![],
            task: Some(task),
        })
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub enum NiUsb6001Input {
    Analog { port: String }
}
impl ChannelDataAquisition for NiUsb6001Input {
    fn read(&mut self) -> Result<Vec<DataPoint>> {
        match self {
            NiUsb6001Input::Analog { port: _ } => {
                Err("Channels for this device must be read all together by the device read method.".into())
            },
        }
    }
}

pub fn create_device(name: String, description: String) -> Result<Device> {
    let hardware = NiUsb6001::new(name.clone())?;
    Ok(Device::new(name, description, Hardware::NiUsb6001(hardware)))
}

pub fn add_channel_analog(device: &mut Device, name: String, port: String) -> Result<()> {
    match &mut device.hardware {
        Hardware::NiUsb6001(hardware) => {
            hardware.inputs.push(NiUsb6001Input::Analog { port: port.clone() });
            let voltage_channel = VoltageChannelBuilder::new(port.to_string())?;
            match &mut hardware.task {
                Some(task) => {
                    task.create_channel(voltage_channel)?;
                },
                None => {
                    return Err("No task available to read".into())
                }
            }
        },
        _ => {
            return Err("This channel can only be added to a NI USB 6001 device.".into())
        }
    }
    
    let channel = Channel::new(
        port.clone(),
        name,
        "V".to_string(),
        "Analog input channel".to_string()
    );
    device.add_channel(channel)?;
    Ok(())
}
