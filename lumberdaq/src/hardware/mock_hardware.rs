use crate::Result;
use crate::datapoint::DataPoint;
use crate::channel::{ Channel, ChannelDataAquisition };
use crate::device::{ Device, DeviceInfo, DeviceInterface };
use crate::hardware::{HardwareDataAquisition, Hardware };
use serde::{Deserialize, Serialize};
use chrono;
use rand::random;

#[derive(Serialize, Deserialize, Clone)]
pub struct MockHardware {
    description: String,
    inputs: Vec<MockHardwareInput>,
}

impl MockHardware {
    pub fn new() -> Result<MockHardware> {
        Ok(MockHardware {
            description: "This is a mock device that uses no hardware. It is used for testing and development purposes.".to_string(),
            inputs: vec![],
        })
    }
}

impl DeviceInterface for MockHardware {
    fn connect(&mut self) {
        println!("Connected to mock device.");
    }
}

impl HardwareDataAquisition for MockHardware {
    fn read(&mut self) -> Result<Vec<Vec<DataPoint>>> {
        let mut readings: Vec<Vec<DataPoint>> = vec![];
        for input in self.inputs.iter_mut() {
            let datapoints = input.read()?;
            readings.push(datapoints);
        }
        Ok(readings)
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub enum MockHardwareInput {
    Random,
    Constant(f64),
}
impl ChannelDataAquisition for MockHardwareInput {
    fn read(&mut self) -> Result<Vec<DataPoint>> {
        match self {
            MockHardwareInput::Random => {
                Ok(vec![DataPoint {
                    datetime: chrono::Utc::now(),
                    value: random(),
                }])
            },
            MockHardwareInput::Constant(value) => {
                Ok(vec![DataPoint {
                    datetime: chrono::Utc::now(),
                    value: *value,
                }])
            }
        }
    }
}

pub fn create_device(name: String, description: String) -> Result<Device> {
    let hardware = MockHardware::new()?;
    Ok(Device::new(name, description, Hardware::MockHardware(hardware)))
}

pub fn add_channel_random(device: &mut Device, name: String) -> Result<()> {
    match &mut device.hardware {
        Hardware::MockHardware(hardware) => {
            hardware.inputs.push(MockHardwareInput::Random);
        },
        _ => {
            return Err("This channel can only be added to a Mock Hardware device.".into())
        }
    }
    let n = device.channels.len();
    let id = format!("c{}", n);
    let channel = Channel::new(id, name, "-".to_string(), "Random number generator".to_string());
    device.add_channel(channel)?;
    Ok(())
}
