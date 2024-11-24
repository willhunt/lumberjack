use crate::Result;
use crate::device::Device;
use crate::storage_csv::{create_csv_file, create_json_file};
// use std::error::Error;
// use crate::storage_hdf::{ create_hdf_file, add_hdf_device, add_hdf_channel };
// use hdf5;

pub struct Daq {
    pub devices: Vec<Device>,
    json_path: std::path::PathBuf,
    csv_path: std::path::PathBuf,
    pub csv_writer: Option<csv::Writer<std::fs::File>>,
    // hdf_path: std::path::PathBuf,
}
impl Daq {
    pub fn new(devices: Vec<Device>, storage_path: std::path::PathBuf) -> Result<Daq> {
        let mut daq = Daq {
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
    
    pub fn connect(&self) {
        for device in self.devices.iter() {
            device.config.connect();
        }
    }

    pub fn init_storage(&mut self) -> Result<()> {
        create_json_file(&self.json_path, self)?;
        self.csv_writer = Some(create_csv_file(&self.csv_path)?);
        
        return Ok(());
    }
}