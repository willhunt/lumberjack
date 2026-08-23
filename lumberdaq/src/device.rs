use crate::Result;
use crate::channel::Channel;
use crate::config::DeviceConfig;
use crate::hardware::{ Hardware, HardwareDataAquisition };
use crate::storage::DataSink;
use serde::{Deserialize, Serialize};
use std::time::{ Duration, Instant };

/// How long to leave a failed device alone before trying it again.
///
/// Retrying every read cycle would be worse than useless: opening a port that
/// is not there is slow, so a dead device would stall every other device on
/// every cycle.
pub const RETRY_INTERVAL: Duration = Duration::from_secs(5);

pub trait DeviceInterface {
    fn connect(&mut self) -> Result<()>;
    // fn read(&mut self) -> Result<()>;
}

#[derive(Serialize, Deserialize, Clone)]
pub struct DeviceInfo {
    pub name: String,
    pub description: String,
}

#[derive(Clone, Debug)]
pub enum ConnectionStatus {
    Connected,
    /// Not usable right now. Carries why, so it can be reported without
    /// guessing, and when it was last tried, so retries stay spaced out.
    /// `last_attempt` is None for a device that has never been tried.
    Disconnected {
        reason: String,
        last_attempt: Option<Instant>,
    },
}

impl Default for ConnectionStatus {
    fn default() -> ConnectionStatus {
        ConnectionStatus::Disconnected {
            reason: "Not connected yet.".to_string(),
            last_attempt: None,
        }
    }
}

/// Whether a device is due another connection attempt.
///
/// A device that has never been tried is due immediately; one that failed is
/// due once `RETRY_INTERVAL` has passed since the last attempt.
fn retry_due(status: &ConnectionStatus) -> bool {
    match status {
        ConnectionStatus::Connected => false,
        ConnectionStatus::Disconnected { last_attempt, .. } => match last_attempt {
            None => true,
            Some(attempt) => attempt.elapsed() >= RETRY_INTERVAL,
        },
    }
}

/// A device as it exists while running: its description, the data acquired so
/// far, and the hardware it is talking to. Not serializable - saving one means
/// asking it for its `DeviceConfig`.
pub struct Device {
    pub info: DeviceInfo,
    pub channels: Vec<Channel>,
    pub hardware: Hardware,
    pub connection: ConnectionStatus,
}

impl Device {
    /// Build a running device from its description.
    ///
    /// Channels go through `add_channel`, so a config with duplicate channel
    /// names is rejected here rather than surfacing later.
    pub fn from_config(config: DeviceConfig) -> Result<Device> {
        let mut device = Device {
            info: config.info,
            channels: vec![],
            hardware: Hardware::from_config(config.hardware)?,
            connection: ConnectionStatus::default(),
        };
        device.rebuild_channels()?;
        Ok(device)
    }

    /// Describe this device so it can be saved.
    ///
    /// The channels are not listed here; they live in the hardware config,
    /// which is the one place they are defined.
    pub fn config(&self) -> DeviceConfig {
        DeviceConfig {
            info: self.info.clone(),
            hardware: self.hardware.config(),
        }
    }

    /// Mirror the hardware's channel definitions onto this device.
    ///
    /// The hardware config decides which channels exist and in what order;
    /// this builds the matching buffers. Discards any data already collected,
    /// so it belongs to setting a device up, not to running it.
    pub fn rebuild_channels(&mut self) -> Result<()> {
        self.channels = vec![];
        for info in self.hardware.config().channel_infos().into_iter() {
            self.add_channel(Channel::from_info(info))?;
        }
        Ok(())
    }

    pub fn new(name: String, description: String, hardware: Hardware) -> Device {
        Device {
            info: DeviceInfo {
                name: name,
                description: description,
            },            
            channels: vec![],
            hardware: hardware,
            connection: ConnectionStatus::default(),
        }
    }

    pub fn add_channel(&mut self, channel: Channel) -> Result<()> {
        for existing_channel in self.channels.iter() {
            if existing_channel.info.name == channel.info.name {
                return Err("Channel name must be unique".into());
            }
        }
        self.channels.push(channel);
        Ok(())
    }

    pub fn print_latest(&self) {
        println!("Latest reading from device: {}", &self.info.name);
        for channel in self.channels.iter() {
            println!("    {}", channel.latest_as_string());
        }
    }

    pub fn is_connected(&self) -> bool {
        matches!(self.connection, ConnectionStatus::Connected)
    }

    /// Why this device is unusable, or None if it is fine.
    pub fn disconnected_reason(&self) -> Option<&str> {
        match &self.connection {
            ConnectionStatus::Connected => None,
            ConnectionStatus::Disconnected { reason, .. } => Some(reason),
        }
    }

    fn mark_disconnected(&mut self, reason: String) {
        self.connection = ConnectionStatus::Disconnected {
            reason: reason,
            last_attempt: Some(Instant::now()),
        };
    }

    /// Read this cycle's data, or quietly work towards being able to.
    ///
    /// A disconnected device is retried at most every `RETRY_INTERVAL` and
    /// otherwise does nothing, so an unplugged cable costs one attempt every
    /// few seconds rather than stalling every cycle.
    ///
    /// A read that fails is treated as having lost the device, so the same
    /// retry path picks it up rather than the error repeating every cycle.
    pub fn read(&mut self) -> Result<()> {
        if !self.is_connected() {
            if retry_due(&self.connection) {
                self.connect()?;
            }
            return Ok(());
        }

        match self.hardware.read() {
            Ok(mut input_readings) => {
                for (channel, datapoints) in self.channels.iter_mut().zip(input_readings.iter_mut()) {
                    channel.add_datapoints(datapoints)?;
                }
                Ok(())
            }
            Err(error) => {
                // Only stand the device down if it has actually gone away. A
                // frame that failed to parse leaves a perfectly good port open,
                // and reconnecting it would lose data for nothing.
                if error.is_connection_lost() {
                    self.mark_disconnected(error.to_string());
                }
                Err(error)
            }
        }
    }

    pub fn write(&mut self, sink: &mut dyn DataSink) -> Result<()>{
        for channel in self.channels.iter_mut() {
            let batch = channel.drain_batch(&self.info.name);
            sink.write_batch(&batch)?;
        }
        Ok(())
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    fn failed_at(when: Option<Instant>) -> ConnectionStatus {
        ConnectionStatus::Disconnected {
            reason: "Port not found.".to_string(),
            last_attempt: when,
        }
    }

    #[test]
    fn a_device_never_tried_is_due_immediately() {
        assert!(retry_due(&ConnectionStatus::default()));
        assert!(retry_due(&failed_at(None)));
    }

    #[test]
    fn a_device_that_just_failed_is_left_alone() {
        assert!(!retry_due(&failed_at(Some(Instant::now()))));
    }

    #[test]
    fn a_device_that_failed_long_enough_ago_is_due_again() {
        let long_ago = Instant::now().checked_sub(RETRY_INTERVAL * 2).unwrap();
        assert!(retry_due(&failed_at(Some(long_ago))));
    }

    #[test]
    fn a_connected_device_is_never_due() {
        assert!(!retry_due(&ConnectionStatus::Connected));
    }

    #[test]
    fn a_new_device_starts_disconnected_with_a_reason() {
        let device = Device::new("Test".to_string(), "-".to_string(), Hardware::None);
        assert!(!device.is_connected());
        assert!(device.disconnected_reason().is_some());
    }

    #[test]
    fn a_failed_connection_is_recorded_on_the_device() {
        // Hardware::None always refuses to connect, which is enough to check
        // that the failure is stored rather than only returned.
        let mut device = Device::new("Test".to_string(), "-".to_string(), Hardware::None);
        // Matching the variant rather than the message: the whole point of a
        // typed error is that callers stop parsing prose.
        let error = device.connect().err().unwrap();
        assert!(matches!(error, crate::Error::NoHardware));
        assert!(!device.is_connected());
        assert!(device.disconnected_reason().is_some());
    }

    #[test]
    fn a_bad_frame_does_not_stand_the_device_down() {
        // A parse failure means the data was wrong, not that the port died, so
        // the device must stay connected rather than being reconnected.
        let error = crate::Error::FieldNotNumeric {
            channel: "Pressure".to_string(),
            index: 5,
            field: "STBY".to_string(),
            frame: "1,2.00,0,1,1,STBY".to_string(),
        };
        assert!(!error.is_connection_lost());
    }

    #[test]
    fn losing_the_port_does_stand_the_device_down() {
        let error = crate::Error::NotConnected { port: "COM3".to_string() };
        assert!(error.is_connection_lost());
        let unplugged = crate::Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "device disconnected",
        ));
        assert!(unplugged.is_connection_lost());
    }

    #[test]
    fn reading_a_disconnected_device_does_not_produce_data() {
        let mut device = Device::new("Test".to_string(), "-".to_string(), Hardware::None);
        // First read is due a retry, which fails and is reported.
        assert!(device.read().is_err());
        // The next read is inside the retry interval, so it does nothing at all
        // rather than hammering the missing device.
        assert!(device.read().is_ok());
        assert!(!device.is_connected());
    }
}

impl DeviceInterface for Device {
    /// Attempt a connection, recording the outcome either way.
    ///
    /// The error is both stored and returned: stored so the device can report
    /// its own state later, returned so a caller trying to connect everything
    /// can collect what went wrong.
    fn connect(&mut self) -> Result<()> {
        match self.hardware.connect() {
            Ok(()) => {
                self.connection = ConnectionStatus::Connected;
                Ok(())
            }
            Err(error) => {
                self.mark_disconnected(error.to_string());
                Err(error)
            }
        }
    }
}