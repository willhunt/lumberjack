use crate::Result;
use crate::device::{ Device, DeviceInterface };
use crate::storage_csv::{ write_csv_file, write_json_file };
use serde::{ Deserialize, Serialize };

#[derive(Serialize, Deserialize, Clone)] // csv_writer connot be cloned
pub struct DaqInfo {
    pub name: String,
    pub author: String,
}

#[derive(Serialize, Deserialize)] // csv_writer connot be cloned
pub struct Daq {
    pub info: DaqInfo,
    pub devices: Vec<Device>,
    pub json_path: std::path::PathBuf,
    pub csv_path: std::path::PathBuf,
    #[serde(skip)]
    pub csv_writer: Option<csv::Writer<std::fs::File>>,
    // hdf_path: std::path::PathBuf,
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
            csv_writer: None,
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

    pub fn init_storage(&mut self) -> Result<()> {
        write_json_file(&self.json_path, self)?;
        self.csv_writer = Some(write_csv_file(&self.csv_path)?);
        
        return Ok(());
    }
}