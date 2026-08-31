//! National Instruments analog input, through the DAQmx driver.
//!
//! Kept for the USB-6001 hardware we already own. New work goes to Pico.
//!
//! Polled only. DAQmx will clock its own acquisition, and the `nidaqmx` crate
//! has the call for it, but nothing here needs that yet and a streaming mode
//! nobody has run is worse than no streaming mode.
//!
//! The driver is loaded at run time, so a project with no NI device in it never
//! touches it and this builds on a machine with no NI software installed.

use crate::channel::ChannelInfo;
use crate::datapoint::DataPoint;
use crate::device::DeviceInterface;
use crate::hardware::HardwareDataAquisition;
use crate::{ Error, Result };
use nidaqmx::{ can_be_differential, differential_partner, Daqmx, Task, Terminal };
use serde::{ Deserialize, Serialize };

/// What a USB-6001 spans, and a sane default for anything else.
fn default_range() -> (f64, f64) {
    (-10.0, 10.0)
}

fn default_single_ended() -> bool {
    true
}

#[derive(Serialize, Deserialize, Clone)]
pub struct NiDaqmxConfig {
    pub description: String,
    /// The name NI MAX gave the hardware, such as `Dev1`.
    ///
    /// DAQmx addresses channels by it, as in `Dev1/ai0`, and it is assigned
    /// outside this program, so it has to be written down rather than found.
    pub device: String,
    pub channels: Vec<NiDaqmxChannel>,
}

/// One channel: what it is, which input it reads, and how it is wired.
#[derive(Serialize, Deserialize, Clone)]
pub struct NiDaqmxChannel {
    #[serde(flatten)]
    pub info: ChannelInfo,
    /// Analog input number, so 0 means `ai0`.
    pub channel: u32,
    /// The volts this input is expected to span, smallest first. The driver
    /// picks a gain from it and refuses a span the hardware cannot reach.
    #[serde(default = "default_range")]
    pub range: (f64, f64),
    /// Single ended measures against the device's ground. Differential pairs
    /// this channel with the one four above it, so on an eight input device
    /// only ai0 to ai3 can start a pair.
    #[serde(default = "default_single_ended")]
    pub single_ended: bool,
}

/// The running device: its settings, and the task once connected.
pub struct NiDaqmx {
    config: NiDaqmxConfig,
    /// Holds the driver alive as well, so nothing else has to.
    task: Option<Task>,
}

impl NiDaqmx {
    pub fn from_config(config: NiDaqmxConfig) -> Result<NiDaqmx> {
        check_channels(&config)?;
        Ok(NiDaqmx { config: config, task: None })
    }

    pub fn config(&self) -> NiDaqmxConfig {
        self.config.clone()
    }

    /// How a channel is named to the driver.
    fn physical(&self, channel: &NiDaqmxChannel) -> String {
        format!("{}/ai{}", self.config.device, channel.channel)
    }
}

/// Everything about a setup that can be decided without the hardware.
///
/// Whether the device exists and has that many inputs needs the driver, and is
/// checked at connect. Everything here is true or false from the config alone,
/// which is what lets `lumberdaq check` catch it from a desk.
fn check_channels(config: &NiDaqmxConfig) -> Result<()> {
    if config.device.trim().is_empty() {
        return Err(Error::NiDeviceNotNamed);
    }

    let mut configured: Vec<u32> = Vec::new();
    for channel in config.channels.iter() {
        if configured.contains(&channel.channel) {
            return Err(Error::NiDuplicateChannel {
                device: config.device.clone(),
                channel: channel.channel,
            });
        }
        configured.push(channel.channel);
    }

    for channel in config.channels.iter() {
        if channel.range.0 >= channel.range.1 {
            return Err(Error::NiRangeNotAscending {
                channel: self_name(config, channel),
                low: channel.range.0,
                high: channel.range.1,
            });
        }
        if channel.single_ended {
            continue;
        }
        // The other half of the pair is consumed by it, so reading it
        // separately would be reading one input as two different things.
        let partner = differential_partner(channel.channel);
        if configured.contains(&partner) {
            return Err(Error::NiDifferentialPartnerInUse {
                device: config.device.clone(),
                primary: channel.channel,
                secondary: partner,
            });
        }
    }
    Ok(())
}

fn self_name(config: &NiDaqmxConfig, channel: &NiDaqmxChannel) -> String {
    format!("{}/ai{}", config.device, channel.channel)
}

impl DeviceInterface for NiDaqmx {
    fn connect(&mut self) -> Result<()> {
        let daqmx = Daqmx::load()?;

        // How many inputs the device has is known only to the driver, and a
        // config written for one model refuses clearly on another.
        let inputs = daqmx.analog_inputs(&self.config.device)?;
        if inputs.is_empty() {
            return Err(Error::NiDeviceNotFound {
                device: self.config.device.clone(),
                available: daqmx.devices().unwrap_or_default().join(", "),
            });
        }

        for channel in self.config.channels.iter() {
            let highest = match channel.single_ended {
                true => channel.channel,
                false => differential_partner(channel.channel),
            };
            if highest as usize >= inputs.len() {
                return Err(Error::NiChannelNotOnDevice {
                    channel: self.physical(channel),
                    device: self.config.device.clone(),
                    inputs: inputs.len(),
                });
            }
            // Checked here rather than left to the driver so the message names
            // the pair, where DAQmx would only say the value was unsupported.
            if !channel.single_ended && !can_be_differential(channel.channel, inputs.len()) {
                return Err(Error::NiDifferentialHasNoPartner {
                    channel: self.physical(channel),
                    partner: differential_partner(channel.channel),
                    inputs: inputs.len(),
                });
            }
        }

        // One task holding every channel, which is what makes them share a
        // reading: DAQmx converts them together and hands back one value each.
        let mut task = daqmx.task("")?;
        for channel in self.config.channels.iter() {
            let terminal = match channel.single_ended {
                true => Terminal::SingleEnded,
                false => Terminal::Differential,
            };
            task.add_voltage_input(&self.physical(channel), terminal, channel.range)?;
        }
        // Started now rather than on the first read, so a setting the hardware
        // will not take is refused while connecting instead of partway through
        // a run.
        task.start()?;
        self.task = Some(task);
        Ok(())
    }
}

impl HardwareDataAquisition for NiDaqmx {
    fn read(&mut self) -> Result<Vec<Vec<DataPoint>>> {
        let task = match &mut self.task {
            Some(task) => task,
            None => return Err(Error::NoHardware),
        };

        let values = task.read_one()?;
        // One timestamp for the lot. The device converts every channel in one
        // scan, so stamping them separately would invent differences that are
        // not in the measurement.
        let at = chrono::Utc::now();
        Ok(values
            .into_iter()
            .map(|value| vec![DataPoint { datetime: at, value: value }])
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(number: u32, single_ended: bool) -> NiDaqmxChannel {
        NiDaqmxChannel {
            info: ChannelInfo {
                name: format!("ai{}", number),
                unit: "V".to_string(),
                description: "-".to_string(),
                scale: None,
            },
            channel: number,
            range: default_range(),
            single_ended: single_ended,
        }
    }

    fn config(channels: Vec<NiDaqmxChannel>) -> NiDaqmxConfig {
        NiDaqmxConfig {
            description: "-".to_string(),
            device: "Dev1".to_string(),
            channels: channels,
        }
    }

    #[test]
    fn a_plain_setup_is_accepted() {
        assert!(check_channels(&config(vec![channel(0, true), channel(1, true)])).is_ok());
    }

    #[test]
    fn a_differential_channel_may_not_also_be_read_on_its_own() {
        // ai0 differential measures between ai0 and ai4, so reading ai4 as well
        // would be reading one input as two different things.
        let error =
            check_channels(&config(vec![channel(0, false), channel(4, true)])).unwrap_err();
        assert!(
            matches!(error, Error::NiDifferentialPartnerInUse { primary: 0, secondary: 4, .. }),
            "{}",
            error
        );
        // ai1 differential takes ai1 and ai5, so ai4 is a different input and
        // reading it as well is fine. It is the partner that is spoken for,
        // not the whole upper half.
        assert!(check_channels(&config(vec![channel(1, false), channel(4, true)])).is_ok());
        assert!(check_channels(&config(vec![channel(1, false), channel(5, true)])).is_err());
    }

    #[test]
    fn the_same_input_cannot_be_configured_twice() {
        let error = check_channels(&config(vec![channel(2, true), channel(2, true)])).unwrap_err();
        assert!(matches!(error, Error::NiDuplicateChannel { channel: 2, .. }), "{}", error);
    }

    #[test]
    fn a_range_has_to_go_upwards() {
        let mut backwards = channel(0, true);
        backwards.range = (10.0, -10.0);
        let error = check_channels(&config(vec![backwards])).unwrap_err();
        assert!(matches!(error, Error::NiRangeNotAscending { .. }), "{}", error);
    }

    #[test]
    fn a_device_has_to_be_named() {
        // DAQmx addresses channels by device name, so without one there is
        // nothing to address, and the failure should not wait for a connection.
        let mut nameless = config(vec![channel(0, true)]);
        nameless.device = "  ".to_string();
        assert!(matches!(
            check_channels(&nameless).unwrap_err(),
            Error::NiDeviceNotNamed
        ));
    }

    #[test]
    fn a_channel_is_named_the_way_daqmx_names_one() {
        let device = NiDaqmx::from_config(config(vec![channel(3, true)])).unwrap();
        assert_eq!(device.physical(&device.config.channels[0]), "Dev1/ai3");
    }
}
