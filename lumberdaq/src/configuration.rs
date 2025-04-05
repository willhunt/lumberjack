use crate::Result;
use crate::device::{DeviceInfo, DeviceDataAquisition};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct ConfigFileDevice {
    pub device_type: DeviceType,
    pub info: DeviceInfo,
    pub config: Box<dyn DeviceDataAquisition>,

}

#[derive(Serialize, Deserialize)]
pub struct ConfigFile {
    pub devices: Vec<ConfigFileDevice>,
}

pub fn read_configuration_file<P: AsRef<Path>>(path: P) -> Result<()> {
    let file = File::open(path)?;
    // Wrap the file reader in BufReader for efficiency.
    let reader = BufReader::new(file);

    let config = serde_json::from_reader(reader)?;

    Ok(())
}
