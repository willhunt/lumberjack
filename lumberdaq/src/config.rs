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
    pub hardware: HardwareConfig,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct DaqConfig {
    pub info: DaqInfo,
    pub devices: Vec<DeviceConfig>,
}
