// Add module 'hardware' in hardware folder
pub mod mock_hardware;
pub mod pico_hrdl;
pub mod serial_stream;

use crate::channel::ChannelInfo;
use crate::datapoint::DataPoint;
use crate::{ Error, Result };
use mock_hardware::{ MockHardware, MockHardwareConfig };
use pico_hrdl::{ PicoHrdl, PicoHrdlConfig };
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
    PicoHrdl(PicoHrdlConfig),
    SerialStream(SerialStreamConfig),
    None,
}

/// How often a device's channels produce a sample.
///
/// Needed to pair samples from different devices: a calculated channel takes
/// each input's nearest sample within half its own period, so that period has
/// to come from somewhere.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SampleRate {
    /// Every read takes a sample, so the device's read interval is the period.
    PerRead,
    /// The hardware keeps its own schedule at this interval, whatever the
    /// device's read interval is.
    Fixed(std::time::Duration),
    /// The device sends when it likes and nothing declares how often, so it can
    /// only be measured from what arrives.
    Unknown,
}

impl HardwareConfig {
    /// How often this hardware produces a sample per channel.
    pub fn sample_rate(&self) -> SampleRate {
        match self {
            HardwareConfig::MockHardware(config) => match config.acquisition {
                mock_hardware::Acquisition::Polled => SampleRate::PerRead,
                mock_hardware::Acquisition::Streaming { sample_interval_ms } => {
                    SampleRate::Fixed(std::time::Duration::from_millis(sample_interval_ms))
                }
            },
            HardwareConfig::PicoHrdl(config) => match config.acquisition {
                pico_hrdl::Acquisition::Polled => SampleRate::PerRead,
                pico_hrdl::Acquisition::Streaming { sample_interval_ms, .. } => {
                    SampleRate::Fixed(std::time::Duration::from_millis(sample_interval_ms))
                }
            },
            // The device streams at whatever rate it was built for, and nothing
            // in the configuration says what that is.
            HardwareConfig::SerialStream(_) => SampleRate::Unknown,
            HardwareConfig::None => SampleRate::Unknown,
        }
    }

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
            HardwareConfig::PicoHrdl(config) => {
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
    PicoHrdl(PicoHrdl),
    SerialStream(SerialStream),
    None,
}

impl Hardware {
    /// Build the running device described by a config.
    pub fn from_config(config: HardwareConfig) -> Result<Hardware> {
        Ok(match config {
            HardwareConfig::MockHardware(config) => Hardware::MockHardware(MockHardware::from_config(config)?),
            HardwareConfig::PicoHrdl(config) => Hardware::PicoHrdl(PicoHrdl::from_config(config)?),
            HardwareConfig::SerialStream(config) => Hardware::SerialStream(SerialStream::from_config(config)?),
            HardwareConfig::None => Hardware::None,
        })
    }

    /// Describe this device so it can be saved. Each backend hands back the
    /// config it is already holding, so the two cannot drift apart.
    pub fn config(&self) -> HardwareConfig {
        match self {
            Hardware::MockHardware(device) => HardwareConfig::MockHardware(device.config()),
            Hardware::PicoHrdl(device) => HardwareConfig::PicoHrdl(device.config()),
            Hardware::SerialStream(device) => HardwareConfig::SerialStream(device.config()),
            Hardware::None => HardwareConfig::None,
        }
    }
}
impl HardwareDataAquisition for Hardware {
    fn read(&mut self) -> Result<Vec<Vec<DataPoint>>> {
        match self {
            Hardware::MockHardware(device) => device.read(),
            Hardware::PicoHrdl(device) => device.read(),
            Hardware::SerialStream(device) => device.read(),
            Hardware::None => Err(Error::NoHardware)
        }
    }
}
impl DeviceInterface for Hardware {
    fn connect(&mut self) -> Result<()> {
        match self {
            Hardware::MockHardware(device) => device.connect()?,
            Hardware::PicoHrdl(device) => device.connect()?,
            Hardware::SerialStream(device) => device.connect()?,
            Hardware::None => return Err(Error::NoHardware)
        }
        Ok(())
    }
}
