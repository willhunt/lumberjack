use crate::channel::ChannelInfo;
use crate::datapoint::DataPoint;
use crate::device::{ Device, DeviceInterface };
use crate::hardware::{ Hardware, HardwareDataAquisition };
use crate::{ Error, Result };
use picolog::hrdl::{ counts_to_volts, ConversionTime, Hrdl, VoltageRange };
use serde::{ Deserialize, Serialize };
use std::time::Duration;

/// A Pico Technology ADC-20 or ADC-24 high resolution logger.
///
/// Channels are read one at a time, each waiting a full conversion. That is
/// slow by the standards of the other backends here - 60ms is the fastest the
/// hardware offers, and eight channels is most of half a second - but it is
/// only slow on this device's own thread, so nothing else waits for it.
///
/// Unlike the serial backend, the channels of one scan do *not* share a
/// timestamp. They genuinely are not simultaneous: the last channel of an
/// eight channel scan is read most of a second after the first, and pretending
/// otherwise would put a lie in the results.

/// Everything needed to describe an ADC-20/24 in a config file.
#[derive(Serialize, Deserialize, Clone)]
pub struct PicoHrdlConfig {
    pub description: String,
    /// How long the converter integrates for, per channel. Longer is quieter
    /// and slower.
    #[serde(default)]
    pub conversion_time: ConversionTime,
    /// Reject 60Hz mains hum rather than 50Hz.
    #[serde(default)]
    pub mains_sixty_hertz: bool,
    pub channels: Vec<PicoHrdlChannel>,
}

/// One channel: what it is, and which input it reads.
#[derive(Serialize, Deserialize, Clone)]
pub struct PicoHrdlChannel {
    #[serde(flatten)]
    pub info: ChannelInfo,
    /// Analog input number, 1 to 16.
    pub channel: u16,
    /// Full scale range for this input.
    pub range: VoltageRange,
    /// Single ended measures against ground. Differential pairs this channel
    /// with the one above it, so only odd numbered channels can be used.
    #[serde(default = "default_single_ended")]
    pub single_ended: bool,
}

fn default_single_ended() -> bool {
    true
}

/// The running device: its settings, and the open unit once connected.
pub struct PicoHrdl {
    config: PicoHrdlConfig,
    unit: Option<Hrdl>,
    /// Full scale counts per channel, in config order, read from the unit at
    /// connect. Needed to turn counts into volts, and it varies with the
    /// conversion time, so it cannot be a constant.
    full_scale_counts: Vec<i32>,
}

impl PicoHrdl {
    pub fn new() -> Result<PicoHrdl> {
        PicoHrdl::from_config(PicoHrdlConfig {
            description: "Pico Technology high resolution data logger.".to_string(),
            conversion_time: ConversionTime::default(),
            mains_sixty_hertz: false,
            channels: vec![],
        })
    }

    pub fn from_config(config: PicoHrdlConfig) -> Result<PicoHrdl> {
        Ok(PicoHrdl {
            config: config,
            unit: None,
            full_scale_counts: vec![],
        })
    }

    pub fn config(&self) -> PicoHrdlConfig {
        self.config.clone()
    }

    pub fn add_channel(&mut self, channel: PicoHrdlChannel) {
        self.config.channels.push(channel);
    }

    /// A lower bound on how fast a full scan can be taken.
    ///
    /// Every channel waits its own conversion, so this is the conversion time
    /// multiplied by the channel count.
    ///
    /// Known to be optimistic. Measured against a real ADC-20 with two channels
    /// at a 60ms conversion, the second channel cost 114ms rather than 60, so
    /// switching input appears to cost about a conversion on top of the
    /// conversion itself. Treat this as a floor that a real scan will exceed,
    /// not as a prediction.
    pub fn minimum_interval(&self) -> Duration {
        Duration::from_millis(
            self.config.conversion_time.millis() * self.config.channels.len().max(1) as u64,
        )
    }
}

impl DeviceInterface for PicoHrdl {
    fn connect(&mut self) -> Result<()> {
        let mut unit = Hrdl::open()?;
        unit.set_mains_rejection(self.config.mains_sixty_hertz)?;

        let mut full_scale_counts = Vec::with_capacity(self.config.channels.len());
        for channel in self.config.channels.iter() {
            unit.enable_channel(channel.channel, channel.range, channel.single_ended)?;
            // Ask the unit rather than assuming: the count range depends on the
            // conversion time, so it is a property of this configuration.
            let (_minimum, maximum) = unit.count_range(channel.channel)?;
            full_scale_counts.push(maximum);
        }

        self.full_scale_counts = full_scale_counts;
        self.unit = Some(unit);
        Ok(())
    }
}

impl HardwareDataAquisition for PicoHrdl {
    fn read(&mut self) -> Result<Vec<Vec<DataPoint>>> {
        let unit = match &self.unit {
            Some(unit) => unit,
            None => return Err(Error::NoHardware),
        };

        let mut readings: Vec<Vec<DataPoint>> = Vec::with_capacity(self.config.channels.len());
        for (channel, full_scale) in self.config.channels.iter().zip(self.full_scale_counts.iter())
        {
            let reading = unit.read_single(
                channel.channel,
                channel.range,
                self.config.conversion_time,
                channel.single_ended,
            )?;
            readings.push(vec![DataPoint {
                // Stamped per channel, after its own conversion, because that
                // is when this value was actually measured.
                datetime: chrono::Utc::now(),
                value: counts_to_volts(reading.counts, *full_scale, channel.range),
            }]);
        }
        Ok(readings)
    }
}

pub fn create_device(name: String, description: String) -> Result<Device> {
    let hardware = PicoHrdl::new()?;
    Ok(Device::new(name, description, Hardware::PicoHrdl(hardware)))
}

pub fn add_channel(
    device: &mut Device,
    name: String,
    description: String,
    channel: u16,
    range: VoltageRange,
    single_ended: bool,
) -> Result<()> {
    match &mut device.hardware {
        Hardware::PicoHrdl(hardware) => {
            hardware.add_channel(PicoHrdlChannel {
                info: ChannelInfo {
                    name: name,
                    unit: "V".to_string(),
                    description: description,
                },
                channel: channel,
                range: range,
                single_ended: single_ended,
            });
        }
        _ => {
            return Err(Error::WrongHardwareType {
                expected: "Pico high resolution logger".to_string(),
            })
        }
    }
    device.rebuild_channels()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_with(channels: usize, conversion: ConversionTime) -> PicoHrdlConfig {
        PicoHrdlConfig {
            description: "-".to_string(),
            conversion_time: conversion,
            mains_sixty_hertz: false,
            channels: (0..channels)
                .map(|index| PicoHrdlChannel {
                    info: ChannelInfo {
                        name: format!("Channel {}", index + 1),
                        unit: "V".to_string(),
                        description: "-".to_string(),
                    },
                    channel: index as u16 + 1,
                    range: VoltageRange::MilliVolts2500,
                    single_ended: true,
                })
                .collect(),
        }
    }

    /// A single channel read on a real ADC-20 cost 57-58ms against a 60ms
    /// conversion, so for one channel this is accurate. With more it is a floor
    /// rather than an estimate: see the note on minimum_interval.
    #[test]
    fn the_scan_floor_is_the_conversion_time_per_channel() {
        let device = PicoHrdl::from_config(config_with(8, ConversionTime::Ms60)).unwrap();
        assert_eq!(device.minimum_interval(), Duration::from_millis(480));

        let slow = PicoHrdl::from_config(config_with(2, ConversionTime::Ms660)).unwrap();
        assert_eq!(slow.minimum_interval(), Duration::from_millis(1320));
    }

    /// A device with no channels still takes some time to talk to, so the floor
    /// must not be zero.
    #[test]
    fn a_device_with_no_channels_has_a_floor() {
        let empty = PicoHrdl::from_config(config_with(0, ConversionTime::Ms60)).unwrap();
        assert_eq!(empty.minimum_interval(), Duration::from_millis(60));
    }

    /// Reading before connecting must say so rather than reaching for a unit
    /// that is not there.
    #[test]
    fn reading_before_connecting_is_rejected() {
        let mut device = PicoHrdl::from_config(config_with(1, ConversionTime::Ms60)).unwrap();
        assert!(matches!(device.read(), Err(Error::NoHardware)));
    }

    /// Settings round trip through a config file, since that is how a rig is
    /// actually described.
    #[test]
    fn settings_survive_a_config_round_trip() {
        let json = r#"{
            "description": "ADC-20",
            "conversion_time": "ms180",
            "mains_sixty_hertz": true,
            "channels": [
                {
                    "name": "Load cell",
                    "unit": "V",
                    "description": "-",
                    "channel": 3,
                    "range": "milli_volts625",
                    "single_ended": false
                }
            ]
        }"#;
        let config: PicoHrdlConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.conversion_time, ConversionTime::Ms180);
        assert!(config.mains_sixty_hertz);
        assert_eq!(config.channels[0].channel, 3);
        assert_eq!(config.channels[0].range, VoltageRange::MilliVolts625);
        assert!(!config.channels[0].single_ended);
    }

    /// Most inputs are single ended, so a config should not have to say so.
    #[test]
    fn single_ended_is_the_default() {
        let json = r#"{
            "description": "ADC-20",
            "channels": [
                { "name": "A", "unit": "V", "description": "-",
                  "channel": 1, "range": "milli_volts2500" }
            ]
        }"#;
        let config: PicoHrdlConfig = serde_json::from_str(json).unwrap();
        assert!(config.channels[0].single_ended);
        assert_eq!(config.conversion_time, ConversionTime::Ms60);
    }
}
