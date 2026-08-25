use crate::calculated::{ CalculatedDevice, ChannelRef };
use crate::daq::DaqInfo;
use crate::device::DeviceInfo;
use crate::hardware::{ HardwareConfig, SampleRate };
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
    /// How often to collect from this device, in milliseconds.
    ///
    /// A collection rate, not a sample rate. For a polled device the two are
    /// the same, because a read takes a sample. For a device that samples on
    /// its own schedule this only decides how often the results are gathered
    /// up for saving; how fast it samples is the hardware's setting, and the
    /// timestamps are the same whatever this is.
    ///
    /// Per device rather than per system: a thermocouple logger sampling once a
    /// second and a serial rig at 50Hz belong in the same test, and each gets
    /// its own thread so neither waits for the other.
    #[serde(default = "default_read_interval_ms", alias = "sample_interval_ms")]
    pub read_interval_ms: u64,
    pub hardware: HardwareConfig,
}

/// 10Hz, matching the serial device this was built against. Also the fallback
/// for configs written before the setting existed.
pub fn default_read_interval_ms() -> u64 {
    100
}

/// Where a project records its results.
///
/// Part of the setup rather than something the caller chooses, so a project
/// directory says what it is. Otherwise the same directory could be run two
/// ways and end up with half the data in each format.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum StorageFormat {
    /// One file holding the setup and every run, queryable while recording.
    #[default]
    Sqlite,
    /// Long format csv plus a json sidecar. Readable by anything, and a run
    /// that dies halfway still leaves a usable file.
    Csv,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct DaqConfig {
    pub info: DaqInfo,
    #[serde(default)]
    pub storage: StorageFormat,
    pub devices: Vec<DeviceConfig>,
    /// Channels worked out from measured ones rather than read from hardware.
    ///
    /// Separate from `devices` because it is not one: it owns no hardware, is
    /// not connected to, and is not read on a thread. It appears in the results
    /// as a device because that is what it is to anyone reading them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calculated: Option<CalculatedDevice>,
}

impl DaqConfig {
    /// Every measured channel, in the form an equation refers to one.
    ///
    /// What a user interface offers when someone is choosing an input for a
    /// calculated channel, so the choice comes from the setup rather than from
    /// them typing a device and channel name correctly.
    pub fn available_inputs(&self) -> Vec<ChannelRef> {
        let mut inputs = Vec::new();
        for device in self.devices.iter() {
            for channel in device.hardware.channel_infos() {
                inputs.push(ChannelRef {
                    device: device.info.name.clone(),
                    channel: channel.name,
                });
            }
        }
        inputs
    }
}

impl DeviceConfig {
    /// How often this device's channels produce a sample, where the setup says.
    ///
    /// None for a device that streams at a rate of its own choosing: it has to
    /// be measured from what arrives.
    pub fn sample_interval(&self) -> Option<std::time::Duration> {
        match self.hardware.sample_rate() {
            SampleRate::Fixed(interval) => Some(interval),
            SampleRate::PerRead => Some(std::time::Duration::from_millis(self.read_interval_ms)),
            SampleRate::Unknown => None,
        }
    }
}
