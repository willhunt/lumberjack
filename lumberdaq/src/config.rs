use crate::daq::DaqInfo;
use crate::device::DeviceInfo;
use crate::hardware::HardwareConfig;
use serde::{ Deserialize, Serialize };

/// The description of a measurement setup: which devices, which channels,
/// which ports. Enough to rebuild the whole thing, and nothing else.
///
/// These types are the file format rather than a second object model. Nothing
/// holds one while running: `Daq::config()` builds one on demand to save, and
/// `Daq::from_config()` consumes one to build the running system. That is why
/// `DeviceInfo` appearing here and in `Device` is not two copies of the same
/// state - at runtime only the `Device` exists.

/// Note there is no channel list here. Channels are defined inside the hardware
/// config, alongside the binding that says where each one's data comes from.
/// Keeping them apart meant two lists matched by position, which a hand edited
/// config could silently put out of order.
#[derive(Serialize, Deserialize, Clone)]
pub struct DeviceConfig {
    pub info: DeviceInfo,
    /// How often to read this device, in milliseconds.
    ///
    /// Per device rather than per system: a thermocouple logger sampling once a
    /// second and a serial rig at 50Hz belong in the same test, and each gets
    /// its own thread so neither waits for the other.
    #[serde(default = "default_sample_interval_ms")]
    pub sample_interval_ms: u64,
    pub hardware: HardwareConfig,
}

/// 10Hz, matching the serial device this was built against. Also the fallback
/// for configs written before the setting existed.
pub fn default_sample_interval_ms() -> u64 {
    100
}

#[derive(Serialize, Deserialize, Clone)]
pub struct DaqConfig {
    pub info: DaqInfo,
    pub devices: Vec<DeviceConfig>,
}
