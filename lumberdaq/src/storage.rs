use crate::Result;
use crate::channel::ChannelInfo;
use crate::config::DaqConfig;
use crate::daq::DaqInfo;
use crate::datapoint::DataPoint;
use crate::device::DeviceInfo;
use serde::{ Deserialize, Serialize };

/// Storage is split into two halves.
///
/// The *header* describes the test: which devices, which channels, what units.
/// It is written once, when recording starts, and is the same regardless of
/// which format the data itself ends up in.
///
/// The *batches* are the data, streamed in as it is acquired. A batch is the
/// datapoints for one channel of one device. Laying those out on disk (long
/// rows, wide rows, columns) is the sink's business, not the caller's.

#[derive(Serialize, Deserialize)]
pub struct DeviceHeader {
    pub info: DeviceInfo,
    pub channels: Vec<ChannelInfo>,
}

#[derive(Serialize, Deserialize)]
pub struct DaqHeader {
    pub info: DaqInfo,
    pub devices: Vec<DeviceHeader>,
}

impl DaqHeader {
    /// Flatten a setup into the shape the csv sidecar wants: names and units,
    /// with the hardware details left out.
    pub fn from_config(config: &DaqConfig) -> DaqHeader {
        DaqHeader {
            info: config.info.clone(),
            devices: config.devices.iter().map(|device|
                DeviceHeader {
                    info: device.info.clone(),
                    channels: device.hardware.channel_infos(),
                }
            ).chain(config.calculated.iter().map(|calculated|
                // Calculated channels are a device as far as results are
                // concerned: they have names, units and values over time.
                DeviceHeader {
                    info: calculated.info.clone(),
                    channels: calculated.channels.iter().map(|c| c.info.clone()).collect(),
                }
            )).collect(),
        }
    }
}

/// The datapoints acquired for a single channel, on its way to storage.
pub struct Batch {
    pub device: String,
    pub channel: String,
    pub datapoints: Vec<DataPoint>,
}

/// Somewhere acquired data can be written to.
///
/// Implementors decide their own on-disk layout and their own metadata story,
/// so swapping csv for parquet or sqlite means adding a type here rather than
/// changing anything that produces data.
pub trait DataSink {
    /// Called once before any data, to record what is being measured.
    ///
    /// This takes the whole setup rather than a flattened header. Names and
    /// units are enough to label a column, but not enough to say whether two
    /// runs measured the same thing: that needs the port, the baud rate and
    /// which field of the frame each channel reads. A sink that only wants the
    /// labels can call `DaqHeader::from_config`.
    fn init(&mut self, config: &DaqConfig) -> Result<()>;

    /// Write one channel's worth of acquired data.
    ///
    /// This does not guarantee the data has reached disk. Call `flush` for that.
    fn write_batch(&mut self, batch: &Batch) -> Result<()>;

    /// Push everything buffered so far out to disk.
    fn flush(&mut self) -> Result<()>;
}

/// Several sinks fed from one run.
///
/// A run has one sink, so recording to more than one place means a sink that is
/// itself several: CSV alongside SQLite, or a display fed the same data as the
/// file it is being written to. `Fanout` is a `DataSink` like any other, so
/// `Daq::set_sink` takes it without knowing the difference.
///
/// ```no_run
/// use lumberdaq::storage::Fanout;
/// # use lumberdaq::config::StorageFormat;
/// # let project = lumberdaq::project::Project::new("my_project");
/// # let mut daq = lumberdaq::open("my_project")?;
/// let sink = Fanout::new()
///     .and("sqlite", project.sink(StorageFormat::Sqlite)?)
///     .and("csv", project.sink(StorageFormat::Csv)?);
/// daq.set_sink(Box::new(sink))?;
/// # Ok::<(), lumberdaq::Error>(())
/// ```
///
/// Every sink is offered the batch even when an earlier one fails, so a sink
/// that has gone wrong cannot starve the others of data. The failure is then
/// reported, which for now ends the run, as one failing sink always has.
/// Whether a display going wrong should stop a recording is a question worth
/// answering when there is a display to try it with.
#[derive(Default)]
pub struct Fanout {
    /// Named so a failure can say which one, since a sink cannot be asked.
    sinks: Vec<(String, Box<dyn DataSink>)>,
}

impl Fanout {
    pub fn new() -> Fanout {
        Fanout { sinks: Vec::new() }
    }

    /// Add a sink, naming it for error messages.
    pub fn and(mut self, name: impl Into<String>, sink: Box<dyn DataSink>) -> Fanout {
        self.sinks.push((name.into(), sink));
        self
    }

    pub fn len(&self) -> usize {
        self.sinks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sinks.is_empty()
    }

    /// Offer every sink the same work, and report afterwards rather than at the
    /// first failure.
    ///
    /// Stopping at the first would mean the sinks after it never saw the batch,
    /// which is the opposite of what recording to two places is for.
    fn each(&mut self, mut action: impl FnMut(&mut dyn DataSink) -> Result<()>) -> Result<()> {
        let mut failures: Vec<(String, crate::Error)> = Vec::new();
        for (name, sink) in self.sinks.iter_mut() {
            if let Err(error) = action(sink.as_mut()) {
                failures.push((name.clone(), error));
            }
        }

        let mut failures = failures.into_iter();
        match failures.next() {
            None => Ok(()),
            Some((name, error)) => {
                let others: Vec<String> =
                    failures.map(|(name, _)| format!("'{}'", name)).collect();
                Err(crate::Error::SinkFailed {
                    sink: format!("'{}'", name),
                    others: match others.is_empty() {
                        true => String::new(),
                        false => format!(", and so did {}", others.join(" and ")),
                    },
                    source: Box::new(error),
                })
            }
        }
    }
}

impl DataSink for Fanout {
    fn init(&mut self, config: &DaqConfig) -> Result<()> {
        self.each(|sink| sink.init(config))
    }

    fn write_batch(&mut self, batch: &Batch) -> Result<()> {
        self.each(|sink| sink.write_batch(batch))
    }

    fn flush(&mut self) -> Result<()> {
        self.each(|sink| sink.flush())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datapoint::DataPoint;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// What each sink was asked to do, shared so it can be read after the
    /// Fanout has taken ownership. `Rc` is enough because a sink never leaves
    /// the thread that collects: `Daq::run` keeps it while the device threads
    /// take the devices.
    type Log = Rc<RefCell<Vec<String>>>;

    struct Spy {
        name: &'static str,
        log: Log,
        /// What this sink does when asked to write, so a failure can be staged.
        fails: bool,
    }

    impl DataSink for Spy {
        fn init(&mut self, _config: &DaqConfig) -> Result<()> {
            self.log.borrow_mut().push(format!("{} init", self.name));
            Ok(())
        }

        fn write_batch(&mut self, batch: &Batch) -> Result<()> {
            self.log.borrow_mut().push(format!("{} {}", self.name, batch.channel));
            match self.fails {
                true => Err(crate::Error::NoFrameFound {
                    port: self.name.to_string(),
                    bytes: 0,
                }),
                false => Ok(()),
            }
        }

        fn flush(&mut self) -> Result<()> {
            self.log.borrow_mut().push(format!("{} flush", self.name));
            Ok(())
        }
    }

    fn spy(name: &'static str, log: &Log, fails: bool) -> Box<dyn DataSink> {
        Box::new(Spy { name: name, log: Rc::clone(log), fails: fails })
    }

    fn batch(channel: &str) -> Batch {
        Batch {
            device: "Rig".to_string(),
            channel: channel.to_string(),
            datapoints: vec![DataPoint { datetime: chrono::Utc::now(), value: 1.0 }],
        }
    }

    #[test]
    fn every_sink_gets_the_data() {
        let log: Log = Rc::new(RefCell::new(Vec::new()));
        let mut fanout = Fanout::new()
            .and("first", spy("first", &log, false))
            .and("second", spy("second", &log, false));
        assert_eq!(fanout.len(), 2);

        fanout.write_batch(&batch("Flow")).unwrap();
        fanout.flush().unwrap();
        assert_eq!(
            log.borrow().as_slice(),
            ["first Flow", "second Flow", "first flush", "second flush"]
        );
    }

    #[test]
    fn a_failing_sink_does_not_starve_the_others() {
        // The whole point of recording to two places: the one still working
        // must be offered the batch even though the other has just refused it.
        let log: Log = Rc::new(RefCell::new(Vec::new()));
        let mut fanout = Fanout::new()
            .and("broken", spy("broken", &log, true))
            .and("working", spy("working", &log, false));

        let error = fanout.write_batch(&batch("Flow")).unwrap_err();
        assert!(log.borrow().contains(&"working Flow".to_string()), "{:?}", log.borrow());
        assert!(error.to_string().contains("'broken'"), "{}", error);
    }

    #[test]
    fn a_failure_says_what_went_wrong_underneath() {
        let log: Log = Rc::new(RefCell::new(Vec::new()));
        let mut fanout = Fanout::new().and("sqlite", spy("sqlite", &log, true));
        let error = fanout.write_batch(&batch("Flow")).unwrap_err();
        // The cause is kept rather than flattened into the message, so a caller
        // walking the chain still reaches what the sink actually said.
        let cause = std::error::Error::source(&error).expect("no source kept");
        assert!(cause.to_string().contains("no complete frame"), "{}", cause);
    }

    #[test]
    fn several_failures_are_all_named() {
        // A full disk fails every sink writing to it at once. Naming one would
        // make that look like a fault in that sink alone.
        let log: Log = Rc::new(RefCell::new(Vec::new()));
        let mut fanout = Fanout::new()
            .and("sqlite", spy("sqlite", &log, true))
            .and("csv", spy("csv", &log, true))
            .and("display", spy("display", &log, true));

        let text = fanout.write_batch(&batch("Flow")).unwrap_err().to_string();
        for name in ["'sqlite'", "'csv'", "'display'"] {
            assert!(text.contains(name), "{} missing from: {}", name, text);
        }
    }

    #[test]
    fn a_fanout_with_nothing_in_it_is_harmless() {
        // How a run with storage turned off would look, rather than a panic.
        let mut fanout = Fanout::new();
        assert!(fanout.is_empty());
        fanout.write_batch(&batch("Flow")).unwrap();
        fanout.flush().unwrap();
    }
}
