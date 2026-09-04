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

#[derive(Serialize, Deserialize, Clone)]
pub struct DaqConfig {
    pub info: DaqInfo,
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

    /// Every channel the setup produces, measured and calculated alike.
    ///
    /// The counterpart to `available_inputs`, and deliberately a different
    /// list. That one answers "what may an equation read", which is measured
    /// channels only: a calculated channel's output goes to the sink rather
    /// than back through the calculator, so one cannot feed another. This one
    /// answers "what is there to look at or record", which includes them —
    /// what a calculated channel works out is usually the quantity somebody
    /// actually wanted, and the last thing to hide from a plot.
    ///
    /// Calculated channels come last, as they do in the tree and in the
    /// results: they are worked out from what precedes them.
    pub fn all_channels(&self) -> Vec<ChannelRef> {
        let mut channels = self.available_inputs();

        if let Some(calculated) = self.calculated.as_ref() {
            for channel in calculated.channels.iter() {
                channels.push(ChannelRef {
                    device: calculated.info.name.clone(),
                    channel: channel.info.name.clone(),
                });
            }
        }
        channels
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
