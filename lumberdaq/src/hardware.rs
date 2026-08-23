// Add module 'hardware' in hardware folder
pub mod mock_hardware;
pub mod serial_stream;

use crate::datapoint::DataPoint;
use crate::Result;
use mock_hardware::{ MockHardware, MockHardwareConfig, MockHardwareInput };
use serial_stream:: { SerialStream, SerialStreamConfig, SerialStreamInput };
use crate::device::DeviceInterface;
use serde::{ Deserialize, Serialize };

pub trait HardwareDataAquisition {
    fn read(&mut self) -> Result<Vec<Vec<DataPoint>>>;
}

/// How a piece of hardware is described in a config file.
///
/// This is the serializable half. It holds settings only, never an open port
/// or a driver handle, so it can be written to disk and read back without any
/// #[serde(skip)] to hide fields that would not survive the trip.
#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "type")]  // Adds "type: MockHardware" identifies to serilaized output, https://serde.rs/enum-representations.html
pub enum HardwareConfig {
    MockHardware(MockHardwareConfig),
    SerialStream(SerialStreamConfig),
    None,
}

/// A piece of hardware as it exists while running, owning whatever resources
/// it needs. Deliberately not Serialize and not Clone.
pub enum Hardware {
    MockHardware(MockHardware),
    SerialStream(SerialStream),
    None,
}

impl Hardware {
    /// Build the running device described by a config.
    pub fn from_config(config: HardwareConfig) -> Result<Hardware> {
        Ok(match config {
            HardwareConfig::MockHardware(config) => Hardware::MockHardware(MockHardware::from_config(config)?),
            HardwareConfig::SerialStream(config) => Hardware::SerialStream(SerialStream::from_config(config)?),
            HardwareConfig::None => Hardware::None,
        })
    }

    /// Describe this device so it can be saved. Each backend hands back the
    /// config it is already holding, so the two cannot drift apart.
    pub fn config(&self) -> HardwareConfig {
        match self {
            Hardware::MockHardware(device) => HardwareConfig::MockHardware(device.config()),
            Hardware::SerialStream(device) => HardwareConfig::SerialStream(device.config()),
            Hardware::None => HardwareConfig::None,
        }
    }
}
impl HardwareDataAquisition for Hardware {
    fn read(&mut self) -> Result<Vec<Vec<DataPoint>>> {
        match self {
            Hardware::MockHardware(device) => device.read(),
            Hardware::SerialStream(device) => device.read(),
            Hardware::None => Err("No hardware is available for this device. Typically this type is used for reading data.".into())
        }
    }
}
impl DeviceInterface for Hardware {
    fn connect(&mut self) -> Result<()> {
        match self {
            Hardware::MockHardware(device) => device.connect()?,
            Hardware::SerialStream(device) => device.connect()?,
            Hardware::None => return Err("No hardware is available for this device. Typically this type is used for reading data.".into())
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
pub enum Input {
    MockHardware(MockHardwareInput),
    SerialStreamInput(SerialStreamInput),
}