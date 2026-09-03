use crate::{ Error, Result };
use crate::config::DaqConfig;
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

    let mut file = File::open(path).map_err(|error| match error.kind() {
        // By far the most likely way to get here is running somewhere that is
        // not a project, so say that rather than repeating the operating
        // system's "cannot find the file specified" with no file named.
        std::io::ErrorKind::NotFound => Error::NoProjectHere {
            directory: describe_parent(path),
        },
        _ => Error::UnreadableFile { path: path.display().to_string(), source: error },
    })?;

    let mut data = String::new();
    file.read_to_string(&mut data)
        .map_err(|error| Error::UnreadableFile {
            path: path.display().to_string(),
            source: error,
        })?;

    // serde reports a line and column, which is not much use without knowing
    // which file they are in.
    serde_json::from_str(&data).map_err(|error| Error::UnreadableConfig {
        path: path.display().to_string(),
        source: error,
    })
}

/// The directory a path sits in, worded for someone reading an error.
fn describe_parent(path: &std::path::Path) -> String {
    match path.parent() {
        Some(parent) if parent.as_os_str().is_empty() => "the current directory".to_string(),
        Some(parent) if parent == std::path::Path::new(".") => "the current directory".to_string(),
        Some(parent) => parent.display().to_string(),
        None => "the current directory".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The commonest mistake is running somewhere that is not a project. The
    /// operating system only says a file was not found, without naming it, so
    /// this has to.
    #[test]
    fn a_directory_with_no_config_says_so_and_says_where() {
        let missing = std::env::temp_dir().join("lumberdaq_no_project").join("config.json");
        let error = read_configuration_file(&missing).err().unwrap();
        assert!(matches!(error, Error::NoProjectHere { .. }));
        let message = error.to_string();
        assert!(message.contains("no config.json"));
        assert!(message.contains("lumberdaq_no_project"));
    }

    /// Running with no argument looks here, and "." is not worth printing.
    #[test]
    fn the_current_directory_is_named_in_words() {
        assert_eq!(describe_parent(std::path::Path::new("config.json")), "the current directory");
        assert_eq!(describe_parent(std::path::Path::new("./config.json")), "the current directory");
        assert_eq!(describe_parent(std::path::Path::new("a/b/config.json")), "a/b");
    }

    /// serde reports a line and column, which needs a file name to be useful.
    #[test]
    fn a_malformed_config_names_the_file() {
        let directory = std::env::temp_dir().join("lumberdaq_bad_config");
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("config.json");
        std::fs::write(&path, "{ not json ]").unwrap();

        let error = read_configuration_file(&path).err().unwrap();
        assert!(matches!(error, Error::UnreadableConfig { .. }));
        assert!(error.to_string().contains("config.json"));
        // The detail is a cause rather than part of the message.
        assert!(std::error::Error::source(&error).is_some());
    }
}

/// Refuse a path that is not the kind of file being asked for.
///
/// Lived with the csv sink until that was dropped. It is about configuration
/// files, which is here.
fn check_file_extension(path: &std::path::PathBuf, extension: &OsStr) -> Result<()> {
    if path.extension() != Some(extension) {
        let error_msg = format!(
            "Incorrect path extension. The extension must be {:?} but {:?} was provided.",
            &extension,
            &path.extension()
        );
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, error_msg).into());
    }
    Ok(())
}
