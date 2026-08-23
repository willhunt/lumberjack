use crate::Result;
use crate::channel::Channel;
use crate::config::DeviceConfig;
use crate::hardware::{ Hardware, HardwareDataAquisition };
use crate::storage::DataSink;
use serde::{Deserialize, Serialize};

pub trait DeviceInterface {
    fn connect(&mut self) -> Result<()>;
    // fn read(&mut self) -> Result<()>;
}

#[derive(Serialize, Deserialize, Clone)]
pub struct DeviceInfo {
    pub name: String,
    pub description: String,
}

#[derive(Clone, Default)]
pub enum ConnectionStatus{
    Connected,
    #[default]
    Unconnected,
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
            connection: ConnectionStatus::Unconnected,
        };
        for info in config.channels.into_iter() {
            device.add_channel(Channel::from_info(info))?;
        }
        Ok(device)
    }

    /// Describe this device so it can be saved. The channel list is projected
    /// from the live channels rather than stored twice.
    pub fn config(&self) -> DeviceConfig {
        DeviceConfig {
            info: self.info.clone(),
            channels: self.channels.iter().map(|channel| channel.info.clone()).collect(),
            hardware: self.hardware.config(),
        }
    }

    pub fn new(name: String, description: String, hardware: Hardware) -> Device {
        Device {
            info: DeviceInfo {
                name: name,
                description: description,
            },            
            channels: vec![],
            hardware: hardware,
            connection: ConnectionStatus::Unconnected,
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

    pub fn read(&mut self) -> Result<()> {
        // TODO: If not connected, attempt to connect and then try to read again maybe?
        match self.connection {
            ConnectionStatus::Connected => {
                let mut input_readings = self.hardware.read()?;
                for (channel, datapoints) in self.channels.iter_mut().zip(input_readings.iter_mut()) {
                    channel.add_datapoints(datapoints)?;
                }
            },
            ConnectionStatus::Unconnected => {
                self.connect()?;
            }
        }

        Ok(())
    }

    pub fn write(&mut self, sink: &mut dyn DataSink) -> Result<()>{
        for channel in self.channels.iter_mut() {
            let batch = channel.drain_batch(&self.info.name);
            sink.write_batch(&batch)?;
        }
        Ok(())
    }

}

impl DeviceInterface for Device {
    fn connect(&mut self) -> Result<()> {
        match self.hardware.connect() {
            Ok(()) => self.connection = ConnectionStatus::Connected,
            Err(e) => {
                self.connection = ConnectionStatus::Unconnected;
                return Err(e);
            },
        }
        Ok(())
    }
}