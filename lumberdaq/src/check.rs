//! Check a project without running it.
//!
//! Everything here already ran as part of starting a recording. The difference
//! is what it does *not* do: nothing is connected to and nothing is written, so
//! a setup can be checked from a desk with none of the hardware to hand, and
//! checking one does not leave a results file behind.
//!
//! Problems are collected a part at a time rather than stopping at the first
//! overall, because three misconfigured devices should take one run of this to
//! find rather than three. Each part still stops at its own first problem: this
//! builds what a run builds rather than being a second implementation of the
//! rules, which could drift from the real one and say a config is fine when it
//! is not.

use crate::calculated::{ CalculatedChannel, ChannelRef };
use crate::config::DaqConfig;
use crate::device::Device;
use crate::Error;

/// One thing that would stop a run, and which part of the setup it is in.
pub struct Problem {
    /// Named as it appears in the config, so it can be found and fixed:
    /// `device 'Rig'`, `calculated channel 'Delta P'`.
    pub part: String,
    pub error: Error,
}

/// What checking a configuration found.
#[derive(Default)]
pub struct CheckReport {
    /// Parts that built without complaint, described.
    pub passed: Vec<String>,
    pub problems: Vec<Problem>,
}

impl CheckReport {
    pub fn new() -> CheckReport {
        CheckReport::default()
    }

    /// Whether a run would get as far as connecting.
    pub fn is_ok(&self) -> bool {
        self.problems.is_empty()
    }

    fn pass(&mut self, part: String) {
        self.passed.push(part);
    }

    fn problem(&mut self, part: String, error: Error) {
        self.problems.push(Problem { part: part, error: error });
    }
}

/// Build everything a run would build, and report what fails.
///
/// Takes the configuration rather than a directory so a program holding one it
/// has not saved yet — a user interface, about to write a config file — can ask
/// the same question of it.
pub fn check_config(config: DaqConfig) -> CheckReport {
    let mut report = CheckReport::new();

    // Taken from the config rather than from the devices that built, so that a
    // device failing does not also make every channel it declares look missing
    // to the calculated channels reading them.
    let available = config.available_inputs();

    for device_config in config.devices.into_iter() {
        let part = format!("device '{}'", device_config.info.name);
        match Device::from_config(device_config) {
            Ok(device) => {
                report.pass(format!("{}, {} channel(s)", part, device.channels.len()))
            }
            Err(error) => report.problem(part, error),
        }
    }

    if let Some(calculated) = config.calculated {
        for channel in calculated.channels.iter() {
            let part = format!("calculated channel '{}'", channel.info.name);
            // The equation first: an input that does not exist matters less
            // than an equation that could never be worked out at all.
            if let Err(error) = channel.validate() {
                report.problem(part, error);
                continue;
            }
            match missing_input(channel, &available) {
                Some(error) => report.problem(part, error),
                None => report.pass(part),
            }
        }
    }

    report
}

/// The first input naming a channel that nothing provides.
///
/// Without this a typo is not an error at all: the channel simply records
/// nothing, for the whole run, and looks like a sensor that was not plugged in.
fn missing_input(channel: &CalculatedChannel, available: &[ChannelRef]) -> Option<Error> {
    channel
        .inputs
        .values()
        .find(|source| !available.contains(source))
        .map(|source| Error::EquationSourceMissing {
            channel: channel.info.name.clone(),
            reads: source.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A setup described the way a config file describes it, so these read as
    /// the thing being checked rather than as a pile of constructors.
    fn config(devices: &str, calculated: &str) -> DaqConfig {
        let text = format!(
            r#"{{ "info": {{ "name": "Test", "author": "-" }},
                  "devices": [{}] {} }}"#,
            devices, calculated
        );
        serde_json::from_str(&text).expect("test config should parse")
    }

    /// A mock device with one channel, optionally scaled.
    fn device(name: &str, channel: &str, scale: &str) -> String {
        format!(
            r#"{{ "info": {{ "name": "{}", "description": "-" }},
                  "read_interval_ms": 100,
                  "hardware": {{ "type": "MockHardware", "description": "-",
                    "acquisition": {{ "mode": "polled" }},
                    "channels": [ {{ "name": "{}", "unit": "-", "description": "-"{},
                                     "input": {{ "Constant": 1.0 }} }} ] }} }}"#,
            name,
            channel,
            match scale.is_empty() {
                true => String::new(),
                false => format!(r#", "scale": "{}""#, scale),
            }
        )
    }

    fn calculated(name: &str, device: &str, channel: &str, equation: &str) -> String {
        format!(
            r#", "calculated": {{ "info": {{ "name": "Derived", "description": "-" }},
                 "channels": [ {{ "name": "{}", "unit": "-", "description": "-",
                   "inputs": {{ "a": {{ "device": "{}", "channel": "{}" }} }},
                   "equation": "{}" }} ] }}"#,
            name, device, channel, equation
        )
    }

    fn parts_that_failed(report: &CheckReport) -> Vec<&str> {
        report.problems.iter().map(|problem| problem.part.as_str()).collect()
    }

    #[test]
    fn a_sound_setup_has_nothing_to_report() {
        let report = check_config(config(&device("Rig", "Flow", "x * 2"), ""));
        assert!(report.is_ok());
        assert_eq!(report.passed.len(), 1);
    }

    #[test]
    fn every_bad_device_is_reported_not_just_the_first() {
        // The whole reason for this: three faults should take one run to find.
        let devices = format!(
            "{}, {}, {}",
            device("First", "A", "v * 2"),
            device("Second", "B", ""),
            device("Third", "C", "wibble(x)")
        );
        let report = check_config(config(&devices, ""));
        assert_eq!(parts_that_failed(&report), ["device 'First'", "device 'Third'"]);
        assert_eq!(report.passed.len(), 1);
    }

    #[test]
    fn a_calculated_channel_reading_nothing_is_caught() {
        let report = check_config(config(
            &device("Rig", "Flow", ""),
            &calculated("Doubled", "Ghost", "Nothing", "a * 2"),
        ));
        assert_eq!(parts_that_failed(&report), ["calculated channel 'Doubled'"]);
        assert!(
            report.problems[0].error.to_string().contains("Ghost/Nothing"),
            "{}",
            report.problems[0].error
        );
    }

    #[test]
    fn a_broken_device_does_not_also_condemn_what_reads_it() {
        // 'Rig' fails to build, so it provides nothing to a list gathered from
        // built devices, and the calculated channel reading it would be
        // reported as pointing at a channel that does not exist. It does exist;
        // the device it is on is misconfigured. One fault, one report.
        let report = check_config(config(
            &device("Rig", "Flow", "v * 2"),
            &calculated("Doubled", "Rig", "Flow", "a * 2"),
        ));
        assert_eq!(parts_that_failed(&report), ["device 'Rig'"]);
        assert!(report.passed.contains(&"calculated channel 'Doubled'".to_string()));
    }

    #[test]
    fn an_equation_that_cannot_be_read_is_reported_before_its_inputs() {
        // Both are wrong here. Which is worth saying is the equation: an input
        // can be repointed, but an equation that will not evaluate is not a
        // configuration mistake so much as an unfinished one.
        let report = check_config(config(
            &device("Rig", "Flow", ""),
            &calculated("Broken", "Ghost", "Nothing", "a * * 2"),
        ));
        assert_eq!(report.problems.len(), 1);
        assert!(
            matches!(report.problems[0].error, Error::InvalidEquation { .. }),
            "{}",
            report.problems[0].error
        );
    }
}
