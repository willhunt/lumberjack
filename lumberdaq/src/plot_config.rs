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
//!
//! This lives in the library rather than in one interface because more than one
//! of them reads it: the terminal monitor and the graphical one both open the
//! same project, and a file format defined twice is a file format that drifts.
//! Nothing here draws anything — it is serde over [`ChannelRef`] and a path.

use crate::calculated::ChannelRef;
use crate::config::DaqConfig;
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
    /// Where the plots sit relative to each other, for an interface that can
    /// arrange them in two dimensions.
    ///
    /// Deliberately beside the list rather than replacing it. `plots` stays the
    /// answer to what plots there are and what is on them, which is all a
    /// terminal drawing them one under another needs; this only says where a
    /// graphical one puts them. An interface that does not understand it reads
    /// the list and ignores this, and one that does can still open a project
    /// that has never been arranged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<PlotLayout>,
}

/// Which way a split divides the space it is given.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SplitAxis {
    /// A horizontal dividing line: one above, one below.
    Horizontal,
    /// A vertical dividing line: one left, one right.
    Vertical,
}

/// How plots are arranged: a tree of splits with a plot at each leaf.
///
/// Plots are named by their number rather than held inline, so this says only
/// where things go and `plots` remains the one definition of what they are.
/// A number here with no matching plot is skipped, and a plot missing from
/// here is still shown — an arrangement going stale should cost a tidy layout,
/// never a plot.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlotLayout {
    Split {
        axis: SplitAxis,
        /// How much of the space the first half gets, from 0 to 1.
        ratio: f32,
        first: Box<PlotLayout>,
        second: Box<PlotLayout>,
    },
    Plot {
        number: usize,
    },
}

impl PlotLayout {
    /// Every plot named in this arrangement, in the order it holds them.
    pub fn numbers(&self) -> Vec<usize> {
        match self {
            PlotLayout::Plot { number } => vec![*number],
            PlotLayout::Split { first, second, .. } => {
                let mut numbers = first.numbers();
                numbers.extend(second.numbers());
                numbers
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct Plot {
    /// The number typed to put a channel on this plot.
    pub number: usize,
    /// What to call the plot, when somebody has named it.
    ///
    /// Absent rather than defaulted to "Plot 3", so a file says what was
    /// chosen and not what a interface happened to display. A reader with no
    /// name to show falls back to the number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub channels: Vec<ChannelRef>,
}

impl Plot {
    /// What to call this plot on screen.
    pub fn display_name(&self) -> String {
        match &self.name {
            Some(name) => name.clone(),
            None => format!("Plot {}", self.number),
        }
    }
}

impl PlotConfig {
    /// The plots this arrangement does not place, and the places it names that
    /// are not plots.
    ///
    /// Both are ordinary: a plot added from the terminal has never been put
    /// anywhere, and one deleted there leaves a hole behind. An interface uses
    /// this to show every plot regardless — appending what the arrangement
    /// missed, and passing over what it cannot find.
    pub fn unplaced(&self) -> (Vec<usize>, Vec<usize>) {
        let placed = match &self.layout {
            Some(layout) => layout.numbers(),
            None => Vec::new(),
        };
        let existing: Vec<usize> = self.plots.iter().map(|plot| plot.number).collect();

        (
            existing.iter().filter(|number| !placed.contains(number)).copied().collect(),
            placed.iter().filter(|number| !existing.contains(number)).copied().collect(),
        )
    }

    /// The channels this layout names that the setup does not have.
    ///
    /// A layout and a rig are separate files on purpose, so they can disagree:
    /// a channel gets renamed or deleted and a plot is left pointing at
    /// something that is gone. That is not a reason to refuse to open either
    /// of them — a rig is still a rig, and the rest of the plots still draw —
    /// so this reports rather than judges, and the caller decides whether to
    /// skip the channel, say so, or offer to remove it.
    ///
    /// Empty means every plot names something real.
    pub fn dangling(&self, config: &DaqConfig) -> Vec<ChannelRef> {
        // Every channel, not only the measured ones: a plot drawing a
        // calculated channel is naming something real, and checking against
        // the equation-input list would call it dangling and offer to remove
        // the very thing the rig was set up to show.
        let known = config.all_channels();
        self.plots
            .iter()
            .flat_map(|plot| plot.channels.iter())
            .filter(|reference| !known.contains(reference))
            .cloned()
            .collect()
    }
}

pub fn path(directory: impl AsRef<Path>) -> PathBuf {
    directory.as_ref().join(FILE)
}

/// Read the layout for a project, or `None` when it has never been saved.
///
/// Not having one is the ordinary case for a project that has only ever been
/// recorded from the command line, so it is not a failure. A file that is there
/// but will not parse is: it was meant to say something.
pub fn read(directory: impl AsRef<Path>) -> Result<Option<PlotConfig>, String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A rig with one device and two channels on it.
    fn rig() -> DaqConfig {
        serde_json::from_str(
            r#"{
              "info": { "name": "Test", "author": "-" },
              "devices": [{
                "info": { "name": "Rig", "description": "-" },
                "hardware": {
                  "type": "MockHardware",
                  "description": "-",
                  "channels": [
                    { "name": "Flow", "unit": "L/min", "description": "-", "input": "Random" },
                    { "name": "Pressure", "unit": "bar", "description": "-", "input": "Random" }
                  ]
                }
              }]
            }"#,
        )
        .expect("test rig should parse")
    }

    /// The same rig with a calculated channel on top of it.
    fn rig_with_calculated() -> DaqConfig {
        serde_json::from_str(
            r#"{
              "info": { "name": "Test", "author": "-" },
              "devices": [{
                "info": { "name": "Rig" },
                "hardware": {
                  "type": "MockHardware",
                  "channels": [
                    { "name": "Flow", "unit": "L/min", "input": "Random" },
                    { "name": "Pressure", "unit": "bar", "input": "Random" }
                  ]
                }
              }],
              "calculated": {
                "info": { "name": "Derived" },
                "channels": [{
                  "name": "Power",
                  "unit": "W",
                  "inputs": { "f": { "device": "Rig", "channel": "Flow" } },
                  "equation": "f * 2"
                }]
              }
            }"#,
        )
        .expect("test rig should parse")
    }

    #[test]
    fn a_plot_of_a_calculated_channel_is_not_dangling() {
        let layout = layout(vec![("Derived", "Power")]);
        assert!(
            layout.dangling(&rig_with_calculated()).is_empty(),
            "a calculated channel is a real channel to plot"
        );
    }

    #[test]
    fn a_calculated_channel_is_not_offered_as_an_equation_input() {
        let config = rig_with_calculated();
        let inputs = config.available_inputs();

        assert!(
            !inputs.iter().any(|input| input.device == "Derived"),
            "one calculated channel cannot feed another, so it is not an input"
        );
        assert_eq!(inputs.len(), 2);
        assert_eq!(config.all_channels().len(), 3);
    }

    fn layout(channels: Vec<(&str, &str)>) -> PlotConfig {
        PlotConfig {
            version: VERSION,
            history_seconds: 60,
            layout: None,
            plots: vec![Plot {
                number: 1,
                name: None,
                channels: channels
                    .into_iter()
                    .map(|(device, channel)| ChannelRef {
                        device: device.to_string(),
                        channel: channel.to_string(),
                    })
                    .collect(),
            }],
        }
    }

    /// The point of keeping the arrangement beside the list rather than
    /// instead of it: a file written before arrangements existed still reads,
    /// and simply has none.
    #[test]
    fn a_file_with_no_arrangement_still_reads() {
        let config: PlotConfig = serde_json::from_str(
            r#"{ "version": 1, "history_seconds": 60,
                 "plots": [ { "number": 1, "channels": [] } ] }"#,
        )
        .expect("a file without a layout should read");

        assert!(config.layout.is_none());
        assert_eq!(config.plots.len(), 1);
    }

    #[test]
    fn an_arrangement_survives_being_written_out_and_read_back() {
        let arrangement = PlotLayout::Split {
            axis: SplitAxis::Vertical,
            ratio: 0.25,
            first: Box::new(PlotLayout::Plot { number: 1 }),
            second: Box::new(PlotLayout::Split {
                axis: SplitAxis::Horizontal,
                ratio: 0.5,
                first: Box::new(PlotLayout::Plot { number: 2 }),
                second: Box::new(PlotLayout::Plot { number: 3 }),
            }),
        };

        let text = serde_json::to_string(&arrangement).unwrap();
        let back: PlotLayout = serde_json::from_str(&text).unwrap();

        assert_eq!(back, arrangement);
        assert_eq!(back.numbers(), vec![1, 2, 3], "in the order it holds them");
    }

    /// What happens when the two interfaces disagree: one adds a plot without
    /// placing it, the other deletes one that was placed.
    #[test]
    fn plots_and_their_arrangement_are_allowed_to_disagree() {
        let mut config = layout(vec![("Rig", "Flow")]);
        config.plots.push(Plot { number: 2, name: None, channels: vec![] });
        config.layout = Some(PlotLayout::Split {
            axis: SplitAxis::Horizontal,
            ratio: 0.5,
            first: Box::new(PlotLayout::Plot { number: 1 }),
            second: Box::new(PlotLayout::Plot { number: 9 }),
        });

        let (unplaced, missing) = config.unplaced();
        assert_eq!(unplaced, vec![2], "a plot nobody has arranged yet");
        assert_eq!(missing, vec![9], "a place whose plot has gone");
    }

    #[test]
    fn a_layout_of_channels_the_rig_has_names_nothing_dangling() {
        let layout = layout(vec![("Rig", "Flow"), ("Rig", "Pressure")]);
        assert!(layout.dangling(&rig()).is_empty());
    }

    /// The case that happens when somebody renames a channel: the layout still
    /// names what it was called, and the plot is drawing nothing.
    #[test]
    fn a_channel_that_was_renamed_is_reported() {
        let layout = layout(vec![("Rig", "Flow"), ("Rig", "Flow rate")]);
        let dangling = layout.dangling(&rig());

        assert_eq!(dangling.len(), 1, "{:?}", dangling);
        assert_eq!(dangling[0].channel, "Flow rate");
    }

    /// A device renamed is the same fault, and has to be caught the same way:
    /// the channel is still called Flow, but not on a device called that.
    #[test]
    fn a_device_that_was_renamed_is_reported() {
        let layout = layout(vec![("Old rig", "Flow")]);
        assert_eq!(layout.dangling(&rig()).len(), 1);
    }
}

pub fn write(directory: impl AsRef<Path>, config: &PlotConfig) -> Result<PathBuf, String> {
    let path = path(directory);
    let text = serde_json::to_string_pretty(config)
        .map_err(|error| format!("could not write plot configuration: {}", error))?;
    std::fs::write(&path, text + "\n")
        .map_err(|error| format!("could not write {}: {}", path.display(), error))?;
    Ok(path)
}
