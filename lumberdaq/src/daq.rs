use crate::Result;
use crate::config::{ DaqConfig, DeviceConfig };
use crate::device::{ Device, DeviceInterface };
use crate::storage::{ DaqHeader, DataSink };
use serde::{ Deserialize, Serialize };

#[derive(Serialize, Deserialize, Clone)]
pub struct DaqInfo {
    pub name: String,
    pub author: String,
}

/// The outcome of trying to connect a whole setup.
#[derive(Clone, Debug)]
pub struct ConnectionReport {
    pub connected: Vec<String>,
    /// Device name and the reason it could not be connected.
    pub failed: Vec<(String, String)>,
}

impl ConnectionReport {
    pub fn all_connected(&self) -> bool {
        self.failed.is_empty()
    }
}

/// A measurement session as it exists while running. Owns the devices and the
/// sink, so it is neither serializable nor cloneable; `config()` is how you
/// get something that is.
pub struct Daq {
    pub info: DaqInfo,
    pub devices: Vec<Device>,
    pub sink: Option<Box<dyn DataSink>>,
}
impl Daq {
    /// Build a whole running system from a saved setup.
    ///
    /// Where results get written is not decided here. That comes from the
    /// `Project` the config was loaded out of, and reaches the Daq as a sink.
    pub fn from_config(config: DaqConfig) -> Result<Daq> {
        let mut devices: Vec<Device> = Vec::new();
        for device_config in config.devices.into_iter() {
            devices.push(Device::from_config(device_config)?);
        }
        Daq::new(config.info.name, config.info.author, devices)
    }

    /// Describe this setup so it can be saved. Contains no measurement data,
    /// no file paths and no open handles - only what is needed to rebuild it.
    pub fn config(&self) -> DaqConfig {
        DaqConfig {
            info: self.info.clone(),
            devices: self.devices.iter().map(|device| device.config()).collect::<Vec<DeviceConfig>>(),
        }
    }

    pub fn new(name: String, author: String, devices: Vec<Device>) -> Result<Daq> {
        let mut daq = Daq {
            info: DaqInfo {
                name: name,
                author: author,
            },
            devices: vec![],
            sink: None,
        };
        // daq.add_device(devices.pop().unwrap());
        for device in devices.into_iter() {
            daq.add_device(device)?;
        }
        Ok(daq)
    }

    pub fn add_device(&mut self, device: Device) -> Result<()> {
        for existing_device in self.devices.iter() {
            if existing_device.info.name == device.info.name {
                return Err("Device name must be unique".into());
            }
        }
        self.devices.push(device);
        Ok(())
    }
    
    /// Try to connect every device, and say what happened.
    ///
    /// Deliberately does not stop at the first failure and does not return
    /// `Result`: one unreachable device should not decide whether the others
    /// get to record. Whether a partial setup is good enough to run is a
    /// judgement for the caller, who can ask the user; all this does is make
    /// sure the caller has the facts to decide with.
    pub fn connect(&mut self) -> ConnectionReport {
        let mut report = ConnectionReport { connected: vec![], failed: vec![] };
        for device in self.devices.iter_mut() {
            match device.connect() {
                Ok(()) => report.connected.push(device.info.name.clone()),
                Err(error) => report.failed.push((device.info.name.clone(), error.to_string())),
            }
        }
        report
    }

    /// Read every device, returning whatever went wrong rather than stopping.
    ///
    /// A disconnected device is not an error here; it retries on its own
    /// schedule and contributes no data. What comes back is genuine trouble:
    /// a read that failed, or a retry that did not take.
    pub fn read(&mut self) -> Vec<(String, String)> {
        let mut problems: Vec<(String, String)> = Vec::new();
        for device in self.devices.iter_mut() {
            if let Err(error) = device.read() {
                problems.push((device.info.name.clone(), error.to_string()));
            }
        }
        problems
    }

    /// Devices that are currently unusable, and why.
    pub fn disconnected(&self) -> Vec<(String, String)> {
        self.devices
            .iter()
            .filter_map(|device| {
                device.disconnected_reason()
                    .map(|reason| (device.info.name.clone(), reason.to_string()))
            })
            .collect()
    }

    /// Attach somewhere to record to, and write the header describing this test.
    ///
    /// Daq does not know or care which format that is.
    pub fn set_sink(&mut self, mut sink: Box<dyn DataSink>) -> Result<()> {
        sink.init(&DaqHeader::from_daq(self))?;
        self.sink = Some(sink);
        Ok(())
    }

    /// Drain every device's acquired data into the sink. Does nothing if no
    /// sink is attached, so acquisition without recording is not an error.
    pub fn write(&mut self) -> Result<()> {
        let sink = match &mut self.sink {
            Some(sink) => sink,
            None => return Ok(()),
        };
        for device in self.devices.iter_mut() {
            device.write(sink.as_mut())?;
        }
        Ok(())
    }

    /// Push buffered data out to disk. How often this is called is what bounds
    /// how much a crash can lose.
    pub fn flush(&mut self) -> Result<()> {
        if let Some(sink) = &mut self.sink {
            sink.flush()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::{ mock_hardware, Hardware };

    /// A mock device always connects; Hardware::None never does. Together they
    /// stand in for a rig where one instrument is unplugged.
    fn one_good_one_bad() -> Daq {
        let mut good = mock_hardware::create_device("Good".to_string(), "-".to_string()).unwrap();
        mock_hardware::add_channel_random(&mut good, "Random".to_string()).unwrap();
        let bad = Device::new("Bad".to_string(), "-".to_string(), Hardware::None);
        Daq::new("Test".to_string(), "-".to_string(), vec![good, bad]).unwrap()
    }

    #[test]
    fn connecting_reports_every_device_rather_than_stopping_at_the_first_failure() {
        let mut daq = one_good_one_bad();
        let report = daq.connect();
        assert_eq!(report.connected, vec!["Good".to_string()]);
        assert_eq!(report.failed.len(), 1);
        assert_eq!(report.failed[0].0, "Bad");
        assert!(!report.all_connected());
    }

    #[test]
    fn a_failed_device_does_not_stop_the_others_recording() {
        let mut daq = one_good_one_bad();
        daq.connect();

        // The failed connect above counts as the attempt, so the bad device is
        // waiting quietly rather than reporting again. What matters is that the
        // good device read regardless.
        let problems = daq.read();
        assert!(problems.is_empty());
        assert_eq!(daq.devices[0].channels[0].datapoints.len(), 1);

        // And it keeps recording on later cycles too.
        daq.read();
        assert_eq!(daq.devices[0].channels[0].datapoints.len(), 2);
    }

    #[test]
    fn a_failed_device_is_not_retried_every_cycle() {
        let mut daq = one_good_one_bad();
        daq.connect();
        // The connect above counts as the attempt, so the next read is inside
        // the retry interval and stays quiet.
        assert!(daq.read().is_empty());
        assert!(daq.read().is_empty());
    }

    #[test]
    fn disconnected_devices_can_be_listed_with_their_reasons() {
        let mut daq = one_good_one_bad();
        daq.connect();
        let down = daq.disconnected();
        assert_eq!(down.len(), 1);
        assert_eq!(down[0].0, "Bad");
        assert!(down[0].1.contains("hardware"));
    }
}