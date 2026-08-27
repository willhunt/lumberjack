//! How plots are laid out, saved beside the project it belongs to.
//!
//! Kept apart from `config.json`, which describes the rig. What a rig measures
//! and what someone happens to be looking at are different things: a setup is
//! still the same setup whether or not anybody has put a channel on a plot.
//!
//! A channel is named by [`ChannelRef`], which is the library's own way of
//! saying which channel a thing reads and is what calculated channels use. One
//! spelling of that across the whole project is worth more than a shorter file.
//!
//! ```json
//! {
//!   "version": 1,
//!   "history_seconds": 60,
//!   "plots": [
//!     { "number": 1, "channels": [ { "device": "Rig", "channel": "Flow" } ] }
//!   ]
//! }
//! ```
//!
//! The plot number is written out rather than taken from the position in the
//! list. It is what somebody typed, and renumbering their plots behind their
//! back to close a gap is the sort of thing that makes a program feel untrusted.

use lumberdaq::calculated::ChannelRef;
use serde::{ Deserialize, Serialize };
use std::path::{ Path, PathBuf };

/// What the file is called inside a project directory.
pub const FILE: &str = "plot_config.json";

/// Bumped when the shape changes in a way an older reader could not cope with.
/// Adding a field does not count; removing or repurposing one does.
pub const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
pub struct PlotConfig {
    pub version: u32,
    /// How long a plot keeps readings for.
    pub history_seconds: u64,
    pub plots: Vec<Plot>,
}

#[derive(Serialize, Deserialize)]
pub struct Plot {
    /// The number typed to put a channel on this plot.
    pub number: usize,
    pub channels: Vec<ChannelRef>,
}

pub fn path(directory: &str) -> PathBuf {
    Path::new(directory).join(FILE)
}

/// Read the layout for a project, or `None` when it has never been saved.
///
/// Not having one is the ordinary case for a project that has only ever been
/// recorded from the command line, so it is not a failure. A file that is there
/// but will not parse is: it was meant to say something.
pub fn read(directory: &str) -> Result<Option<PlotConfig>, String> {
    let path = path(directory);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("could not read {}: {}", path.display(), error)),
    };
    let config: PlotConfig = serde_json::from_str(&text)
        .map_err(|error| format!("{} is not valid plot configuration: {}", path.display(), error))?;
    if config.version > VERSION {
        return Err(format!(
            "{} was written by a newer version ({}, this understands {})",
            path.display(),
            config.version,
            VERSION
        ));
    }
    Ok(Some(config))
}

pub fn write(directory: &str, config: &PlotConfig) -> Result<PathBuf, String> {
    let path = path(directory);
    let text = serde_json::to_string_pretty(config)
        .map_err(|error| format!("could not write plot configuration: {}", error))?;
    std::fs::write(&path, text + "\n")
        .map_err(|error| format!("could not write {}: {}", path.display(), error))?;
    Ok(path)
}
