use crate::Result;
use crate::device::{ Device, DeviceInterface };
use crate::storage::{ DaqHeader, DataSink };
use serde::{ Deserialize, Serialize };

#[derive(Serialize, Deserialize, Clone)] // csv_writer connot be cloned
pub struct DaqInfo {
    pub name: String,
    pub author: String,
}

#[derive(Serialize, Deserialize)] // a live sink cannot be cloned or serialized
pub struct Daq {
    pub info: DaqInfo,
    pub devices: Vec<Device>,
    pub json_path: std::path::PathBuf,
    pub csv_path: std::path::PathBuf,
    #[serde(skip)]
    pub sink: Option<Box<dyn DataSink>>,
}
impl Daq {
    pub fn new(name: String, author: String, devices: Vec<Device>, storage_path: std::path::PathBuf) -> Result<Daq> {
        let mut daq = Daq {
            info: DaqInfo {
                name: name,
                author: author,     
            },
            devices: vec![],
            json_path: storage_path.clone().with_extension("json"),
            csv_path: storage_path,
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