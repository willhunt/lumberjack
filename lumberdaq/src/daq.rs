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
    
    pub fn connect(&mut self) -> Result<()> {
        for device in self.devices.iter_mut() {
            device.connect()?;
        }
        Ok(())
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