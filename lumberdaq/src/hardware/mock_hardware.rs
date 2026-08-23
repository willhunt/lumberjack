use crate::{ Error, Result };
use crate::datapoint::DataPoint;
use crate::channel::ChannelInfo;
use crate::device::{ Device, DeviceInterface };
use crate::hardware::{HardwareDataAquisition, Hardware };
use serde::{Deserialize, Serialize};
use chrono;
use rand::random;

/// Everything needed to describe a mock device in a config file.
#[derive(Serialize, Deserialize, Clone)]
pub struct MockHardwareConfig {
    pub description: String,
    pub channels: Vec<MockHardwareChannel>,
}

/// One channel: what it is, and what generates its values.
///
/// Description and binding live together so they cannot be listed in different
/// orders, which is what used to decide silently whose data went where.
#[derive(Serialize, Deserialize, Clone)]
pub struct MockHardwareChannel {
    #[serde(flatten)]
    pub info: ChannelInfo,
    pub input: MockHardwareInput,
}

impl Default for MockHardwareConfig {
    fn default() -> MockHardwareConfig {
        MockHardwareConfig {
            description: "This is a mock device that uses no hardware. It is used for testing and development purposes.".to_string(),
            channels: vec![],
        }
    }
}

/// The running device. Mock owns no hardware resource, so it is only its
/// config; the wrapper exists so every backend has the same shape.
pub struct MockHardware {
    config: MockHardwareConfig,
}

impl MockHardware {
    pub fn new() -> Result<MockHardware> {
        MockHardware::from_config(MockHardwareConfig::default())
    }

    pub fn from_config(config: MockHardwareConfig) -> Result<MockHardware> {
        Ok(MockHardware { config: config })
    }

    pub fn config(&self) -> MockHardwareConfig {
        self.config.clone()
    }

    pub fn add_channel(&mut self, channel: MockHardwareChannel) {
        self.config.channels.push(channel);
    }
}

impl DeviceInterface for MockHardware {
    fn connect(&mut self) -> Result<()> {
        println!("Connected to mock device.");
        Ok(())
    }
}

impl HardwareDataAquisition for MockHardware {
    fn read(&mut self) -> Result<Vec<Vec<DataPoint>>> {
        let mut readings: Vec<Vec<DataPoint>> = vec![];
        for channel in self.config.channels.iter_mut() {
            let datapoints = channel.input.read()?;
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
impl MockHardwareInput {
    /// Produce this input's next value.
    ///
    /// Inherent rather than a trait method: the mock is the only hardware that
    /// can produce a single channel on demand. Real instruments hand over every
    /// channel in one transaction, and a backend that does poll channels
    /// individually can simply loop inside its own device level `read`.
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
            hardware.add_channel(MockHardwareChannel {
                info: ChannelInfo {
                    name: name,
                    unit: "-".to_string(),
                    description: "Random number generator".to_string(),
                },
                input: MockHardwareInput::Random,
            });
        },
        _ => {
            return Err(Error::WrongHardwareType { expected: "mock hardware".to_string() })
        }
    }
    // The hardware config is the definition; the device mirrors it.
    device.rebuild_channels()
}
