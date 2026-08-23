pub mod hardware;
mod error;
pub use self::error::{Error, Result};
pub mod datapoint;
pub mod channel;
pub mod device;
pub mod daq;
pub mod config;
pub mod project;
pub mod session;
pub mod storage;
pub mod storage_csv;
pub mod storage_sqlite;

/// Build the system described by a project directory, ready to connect.
///
/// The short way in for a program embedding this library:
///
/// ```no_run
/// let mut daq = lumberdaq::open("my_project")?;
/// let report = daq.connect();
/// # Ok::<(), lumberdaq::Error>(())
/// ```
pub fn open(directory: impl AsRef<std::path::Path>) -> Result<daq::Daq> {
    project::Project::new(directory).open()
}
pub mod configuration;