use crate::{ Error, Result };
use crate::calculated::ChannelRef;
use crate::check::{ check_config, CheckReport };
use crate::config::{ DaqConfig, StorageFormat };
use crate::plot_config::PlotConfig;
use crate::configuration::{ read_configuration_file, write_configuration_file };
use crate::daq::Daq;
use crate::storage::DataSink;
use crate::storage_sqlite::SqliteSink;
use std::path::{ Path, PathBuf };

/// A directory holding one measurement setup and the results recorded from it.
///
/// ```text
/// my_project/
///     config.json       the setup: devices, channels, ports
///     plot_config.json  how somebody is looking at it, if anybody has said
///     results.db        every run ever recorded here
///     export/           one csv per run, written by `lumberdaq export`
/// ```
///
/// The layout is separate from the setup because they are different things: a
/// rig is the same rig whether or not anybody has put a channel on a plot, and
/// a layout changes every time somebody drags one there. Reading both through
/// here is what keeps them agreeing without making them one file.
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

    pub fn database_path(&self) -> PathBuf {
        self.directory.join("results.db")
    }

    /// Where exported CSV files go.
    ///
    /// A directory of its own, since there is one file per run and they
    /// accumulate. Keeping them out of the project directory means what a
    /// project *is* stays legible next to what has been got out of it.
    pub fn export_path(&self) -> PathBuf {
        self.directory.join("export")
    }

    /// How this project's plots are laid out, if anybody has saved a layout.
    pub fn layout_path(&self) -> PathBuf {
        crate::plot_config::path(&self.directory)
    }

    pub fn read_config(&self) -> Result<DaqConfig> {
        read_configuration_file(&self.config_path())
    }

    /// The saved plot layout, or `None` where there is not one.
    ///
    /// Kept a separate file from the rig it belongs to, and read through here
    /// so that every interface finds it the same way rather than each deciding
    /// for itself where a layout lives.
    pub fn read_layout(&self) -> Result<Option<PlotConfig>> {
        crate::plot_config::read(&self.directory).map_err(Error::from)
    }

    /// Save the plot layout beside the rig, returning where it went.
    pub fn write_layout(&self, layout: &PlotConfig) -> Result<PathBuf> {
        crate::plot_config::write(&self.directory, layout).map_err(Error::from)
    }

    /// Which channels the saved layout names that the setup does not have.
    ///
    /// The two files can disagree — a channel renamed in one and not the other
    /// — and this is where that is found out, so both interfaces report it the
    /// same way instead of each checking for itself. No layout and no rig
    /// problem both come back empty.
    pub fn dangling_plot_channels(&self) -> Result<Vec<ChannelRef>> {
        let Some(layout) = self.read_layout()? else {
            return Ok(Vec::new());
        };
        Ok(layout.dangling(&self.read_config()?))
    }

    /// Build the whole system this directory describes, ready to connect.
    ///
    /// This is the entry point for anything embedding lumberdaq: hand it a
    /// directory and get back something that can record. Which storage format
    /// to use comes from the config rather than the caller, so two programs
    /// pointed at the same project cannot disagree about where the data goes.
    ///
    /// Connecting and running are left to the caller. A program wants to see
    /// which devices failed before deciding whether the run is worth starting,
    /// and wants events while it is going.
    pub fn open(&self) -> Result<Daq> {
        let config = self.read_config()?;
        let storage = config.storage;
        let mut daq = Daq::from_config(config)?;
        daq.set_sink(self.sink(storage)?)?;
        Ok(daq)
    }

    /// Check this project's configuration without running it.
    ///
    /// Fails only when there is nothing to check: no config file, or one that
    /// is not valid JSON. Anything past that comes back as a report, since a
    /// bad device is a finding rather than a reason to stop looking.
    pub fn check(&self) -> Result<CheckReport> {
        Ok(check_config(self.read_config()?))
    }

    /// The sink this project records to, per its config.
    pub fn sink(&self, format: StorageFormat) -> Result<Box<dyn DataSink>> {
        Ok(match format {
            StorageFormat::Sqlite => Box::new(SqliteSink::new(&self.database_path())?),
        })
    }

    /// A sink for one recording among several in a session.
    ///
    /// A database keeps every run in the one file and its runs table tells them
    /// apart, so the path does not change. A CSV has no such thing, so each
    /// recording is given a file of its own named by `label`, along with the
    /// sidecar describing it.
    pub fn sink_for(&self, format: StorageFormat, label: &str) -> Result<Box<dyn DataSink>> {
        let _ = label;
        Ok(match format {
            StorageFormat::Sqlite => Box::new(SqliteSink::new(&self.database_path())?),
        })
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
        assert_eq!(project.database_path(), PathBuf::from("some/where/results.db"));
        assert_eq!(project.export_path(), PathBuf::from("some/where/export"));
    }

    #[test]
    fn moving_the_directory_moves_every_path() {
        let moved = Project::new("elsewhere");
        assert_eq!(moved.config_path(), PathBuf::from("elsewhere/config.json"));
        assert_eq!(moved.database_path(), PathBuf::from("elsewhere/results.db"));
    }
}
