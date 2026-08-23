use crate::Result;
use crate::config::DaqConfig;
use crate::storage_csv::check_file_extension;
use std::fs::File;
use std::io::{ Read, Write }; // ErrorKind
// use std::io::BufReader;
// use std::path::Path;
// use serde::{Deserialize, Serialize};
use serde_json::to_string_pretty;
use std::ffi::OsStr;

// #[derive(Serialize, Deserialize)]
// pub struct ConfigFileDevice {
//     pub device_type: DeviceType,
//     pub info: DeviceInfo,
// }

// #[derive(Serialize, Deserialize)]
// pub struct ConfigFile {
//     pub devices: Vec<ConfigFileDevice>,
// }

// pub fn read_configuration_file<P: AsRef<Path>>(path: P) -> Result<()> {
//     let file = File::open(path)?;
//     // Wrap the file reader in BufReader for efficiency.
//     let reader = BufReader::new(file);

//     let config = serde_json::from_reader(reader)?;

//     Ok(())
// }

pub fn write_configuration_file(path: &std::path::PathBuf, config: &DaqConfig) -> Result<()> {
    check_file_extension(path, OsStr::new("json"))?;
    let mut file = File::create(path)?;
    file.write_all(to_string_pretty(config)?.as_bytes())?;
    return Ok(());
}

/// Read a saved setup. This returns a `DaqConfig`, not a `Daq`: what comes off
/// disk is a description, and turning it into something connected to hardware
/// is `Daq::from_config`.
pub fn read_configuration_file(path: &std::path::PathBuf) -> Result<DaqConfig> {
    check_file_extension(path, OsStr::new("json"))?;
    let mut file = File::open(path)?;
    let mut data = String::new();
    file.read_to_string(&mut data)?;

    let config: DaqConfig = serde_json::from_str(&data)?;
    return Ok(config);
}