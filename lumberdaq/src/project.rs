use crate::Result;
use crate::config::DaqConfig;
use crate::configuration::{ read_configuration_file, write_configuration_file };
use std::path::{ Path, PathBuf };

/// A directory holding one measurement setup and the results recorded from it.
///
/// ```text
/// my_project/
///     config.json     the setup: devices, channels, ports
///     results.csv     the data
///     results.json    the header describing that data
/// ```
///
/// The directory is deliberately not recorded inside config.json. A project is
/// wherever its files are, so the whole folder can be moved, copied or handed
/// to someone else and still resolve. Storing the path inside a file that
/// lives at that path means the two can disagree, and the file always loses.
pub struct Project {
    directory: PathBuf,
}

impl Project {
    /// Refer to a project directory. Does not touch the filesystem.
    pub fn new(directory: impl AsRef<Path>) -> Project {
        Project { directory: directory.as_ref().to_path_buf() }
    }

    /// Refer to a project directory, creating it if it does not exist yet.
    pub fn create(directory: impl AsRef<Path>) -> Result<Project> {
        let project = Project::new(directory);
        std::fs::create_dir_all(&project.directory)?;
        Ok(project)
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn config_path(&self) -> PathBuf {
        self.directory.join("config.json")
    }

    pub fn results_path(&self) -> PathBuf {
        self.directory.join("results.csv")
    }

    /// The sidecar describing what is in the results file.
    pub fn header_path(&self) -> PathBuf {
        self.directory.join("results.json")
    }

    pub fn read_config(&self) -> Result<DaqConfig> {
        read_configuration_file(&self.config_path())
    }

    pub fn write_config(&self, config: &DaqConfig) -> Result<()> {
        write_configuration_file(&self.config_path(), config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_derived_from_the_directory() {
        let project = Project::new("some/where");
        assert_eq!(project.config_path(), PathBuf::from("some/where/config.json"));
        assert_eq!(project.results_path(), PathBuf::from("some/where/results.csv"));
        assert_eq!(project.header_path(), PathBuf::from("some/where/results.json"));
    }

    #[test]
    fn moving_the_directory_moves_every_path() {
        let moved = Project::new("elsewhere");
        assert_eq!(moved.config_path(), PathBuf::from("elsewhere/config.json"));
        assert_eq!(moved.results_path(), PathBuf::from("elsewhere/results.csv"));
    }
}
