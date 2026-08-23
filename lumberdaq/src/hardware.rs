// Add module 'hardware' in hardware folder
pub mod mock_hardware;
pub mod serial_stream;

use crate::channel::ChannelInfo;
use crate::datapoint::DataPoint;
use crate::{ Error, Result };
use mock_hardware::{ MockHardware, MockHardwareConfig };
use serial_stream:: { SerialStream, SerialStreamConfig };
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

impl HardwareConfig {
    /// The channels this hardware provides, in the order it reads them.
    ///
    /// The hardware config is the single definition of which channels exist.
    /// A device mirrors this list rather than keeping its own, so the two
    /// cannot disagree about what is being measured or in what order.
    pub fn channel_infos(&self) -> Vec<ChannelInfo> {
        match self {
            HardwareConfig::MockHardware(config) => {
                config.channels.iter().map(|channel| channel.info.clone()).collect()
            }
            HardwareConfig::SerialStream(config) => {
                config.channels.iter().map(|channel| channel.info.clone()).collect()
            }
            HardwareConfig::None => vec![],
        }
    }
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
            Hardware::None => Err(Error::NoHardware)
        }
    }
}
impl DeviceInterface for Hardware {
    fn connect(&mut self) -> Result<()> {
        match self {
            Hardware::MockHardware(device) => device.connect()?,
            Hardware::SerialStream(device) => device.connect()?,
            Hardware::None => return Err(Error::NoHardware)
        }
        Ok(())
    }
}
