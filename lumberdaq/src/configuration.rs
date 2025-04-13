use crate::Result;
use crate::device::DeviceInterface;
use crate::hardware::{ Hardware, Input };
use crate::storage_csv::check_file_extension;
use crate::daq::Daq;
use std::fs::File;
use std::io::{ ErrorKind, Read, Write };
use std::io::BufReader;
use std::path::Path;
use serde::{Deserialize, Serialize};
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

pub fn write_configuration_file(path: &std::path::PathBuf, daq: &Daq) -> Result<()> {
    check_file_extension(path, OsStr::new("json"))?;
    let mut file = File::create(path)?;
    file.write_all(to_string_pretty(&daq)?.as_bytes())?;
    return Ok(());
}

pub fn read_configuration_file(path: &std::path::PathBuf) -> Result<Daq> {
    check_file_extension(path, OsStr::new("json"))?;
    let mut file = File::open(path)?;
    let mut data = String::new();
    file.read_to_string(&mut data)?;

    let daq: Daq = serde_json::from_str(&data)?;
    return Ok(daq);
}