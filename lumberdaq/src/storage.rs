//! Data reaches storage as *batches*: the datapoints for one channel of one
//! device, streamed in as they are acquired. Laying those out on disk is the
//! sink's business, not the caller's.
//!
//! What is being measured reaches it once, as the whole `DaqConfig`, when
//! recording starts.

use crate::Result;
use crate::config::DaqConfig;
use crate::datapoint::DataPoint;
use std::sync::atomic::{ AtomicBool, Ordering };
use std::sync::Arc;

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
    /// This takes the whole setup rather than a flattened list of names. Names
    /// and units are enough to label a column, but not enough to say whether
    /// two runs measured the same thing: that needs the port, the baud rate and
    /// which field of the frame each channel reads.
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
/// itself several: a second copy on another disk, or a display fed the same
/// data as the file it is being written to. `Fanout` is a `DataSink` like any other, so
/// `Daq::set_sink` takes it without knowing the difference.
///
/// ```no_run
/// use lumberdaq::storage::Fanout;
/// # use lumberdaq::storage_sqlite::SqliteSink;
/// # use std::path::PathBuf;
/// # let project = lumberdaq::project::Project::new("my_project");
/// # let mut daq = lumberdaq::open("my_project")?;
/// let sink = Fanout::new()
///     .and("database", project.sink()?)
///     .and("copy", Box::new(SqliteSink::new(&PathBuf::from("elsewhere.db"))?));
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


/// A sink that writes only while it is armed.
///
/// A run holds its sink for the whole run, so nothing can hand one over partway
/// through when somebody presses record. This is attached from the start
/// instead and does nothing until the flag is set, which from the outside comes
/// to the same thing.
///
/// The sink underneath is built on arming rather than up front, so a session
/// that is only being watched leaves no results file behind at all. It is built
/// afresh each time, so stopping and starting again gives a second recording
/// rather than more of the first: what that means is the caller's to decide,
/// since a database can keep both runs in one file while a CSV needs another
/// file.
///
/// ```no_run
/// use lumberdaq::storage::Recorder;
/// # use std::sync::atomic::AtomicBool;
/// # use std::sync::Arc;
/// let recording = Arc::new(AtomicBool::new(false));
/// let project = lumberdaq::project::Project::new("my_project");
/// let recorder = Recorder::new(
///     Arc::clone(&recording),
///     Box::new(move || project.sink()),
/// );
/// ```
pub struct Recorder {
    armed: Arc<AtomicBool>,
    /// Built on arming, dropped on stopping. `None` whenever not recording.
    sink: Option<Box<dyn DataSink>>,
    make: Box<dyn FnMut() -> Result<Box<dyn DataSink>>>,
    /// Kept from `init`, because the sink that needs telling what is being
    /// measured does not exist yet, and may never.
    config: Option<DaqConfig>,
    started: usize,
}

impl Recorder {
    pub fn new(
        armed: Arc<AtomicBool>,
        make: Box<dyn FnMut() -> Result<Box<dyn DataSink>>>,
    ) -> Recorder {
        Recorder { armed: armed, sink: None, make: make, config: None, started: 0 }
    }

    /// How many recordings have been started since the run began.
    pub fn started(&self) -> usize {
        self.started
    }

    /// Whether anything is being written at this moment.
    pub fn recording(&self) -> bool {
        self.sink.is_some()
    }

    /// Bring the sink into line with the flag.
    ///
    /// Called from both `write_batch` and `flush`, because a run being watched
    /// with every device disconnected produces no batches at all, and pressing
    /// record then would otherwise do nothing until data arrived.
    fn follow(&mut self) -> Result<()> {
        match (self.armed.load(Ordering::Relaxed), self.sink.is_some()) {
            (true, false) => {
                let mut sink = (self.make)()?;
                if let Some(config) = &self.config {
                    sink.init(config)?;
                }
                self.sink = Some(sink);
                self.started += 1;
            }
            (false, true) => {
                // Stopping flushes. Whatever is buffered belongs to the
                // recording that has just ended, not to the next one.
                if let Some(mut sink) = self.sink.take() {
                    sink.flush()?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

impl DataSink for Recorder {
    fn init(&mut self, config: &DaqConfig) -> Result<()> {
        self.config = Some(config.clone());
        Ok(())
    }

    fn write_batch(&mut self, batch: &Batch) -> Result<()> {
        self.follow()?;
        match &mut self.sink {
            Some(sink) => sink.write_batch(batch),
            // Watching rather than recording. Not an error: it is the whole
            // point of the thing.
            None => Ok(()),
        }
    }

    fn flush(&mut self) -> Result<()> {
        self.follow()?;
        match &mut self.sink {
            Some(sink) => sink.flush(),
            None => Ok(()),
        }
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

    /// A recorder whose sink is a spy, counting how many were built.
    fn recorder(armed: &Arc<AtomicBool>, log: &Log, made: &Rc<RefCell<usize>>) -> Recorder {
        let log = Rc::clone(log);
        let made = Rc::clone(made);
        Recorder::new(
            Arc::clone(armed),
            Box::new(move || {
                *made.borrow_mut() += 1;
                Ok(spy("recording", &log, false))
            }),
        )
    }

    fn config() -> DaqConfig {
        serde_json::from_str(
            r#"{ "info": { "name": "Test", "author": "-" }, "devices": [] }"#,
        )
        .unwrap()
    }

    fn batch(channel: &str) -> Batch {
        Batch {
            device: "Rig".to_string(),
            channel: channel.to_string(),
            datapoints: vec![DataPoint { datetime: chrono::Utc::now(), value: 1.0 }],
        }
    }

    #[test]
    fn a_recorder_writes_nothing_until_it_is_armed() {
        // A session only being watched should leave no results file behind, so
        // the sink underneath is not even built.
        let log: Log = Rc::new(RefCell::new(Vec::new()));
        let armed = Arc::new(AtomicBool::new(false));
        let made = Rc::new(RefCell::new(0));
        let mut recorder = recorder(&armed, &log, &made);

        recorder.write_batch(&batch("Flow")).unwrap();
        recorder.flush().unwrap();
        assert!(log.borrow().is_empty(), "{:?}", log.borrow());
        assert_eq!(*made.borrow(), 0, "nothing should have been built");
        assert_eq!(recorder.started(), 0);
        assert!(!recorder.recording());
    }

    #[test]
    fn arming_builds_the_sink_and_tells_it_what_is_being_measured() {
        let log: Log = Rc::new(RefCell::new(Vec::new()));
        let armed = Arc::new(AtomicBool::new(false));
        let made = Rc::new(RefCell::new(0));
        let mut recorder = recorder(&armed, &log, &made);
        recorder.init(&config()).unwrap();

        armed.store(true, Ordering::Relaxed);
        recorder.write_batch(&batch("Flow")).unwrap();

        // init on a sink built long after the run started, or it would be
        // writing data with no header to say what any of it is.
        assert_eq!(log.borrow().as_slice(), ["recording init", "recording Flow"]);
        assert_eq!(recorder.started(), 1);
        assert!(recorder.recording());
    }

    #[test]
    fn stopping_flushes_and_lets_go_of_the_sink() {
        let log: Log = Rc::new(RefCell::new(Vec::new()));
        let armed = Arc::new(AtomicBool::new(true));
        let made = Rc::new(RefCell::new(0));
        let mut recorder = recorder(&armed, &log, &made);
        recorder.write_batch(&batch("Flow")).unwrap();

        armed.store(false, Ordering::Relaxed);
        recorder.flush().unwrap();
        // Buffered data belongs to the recording that just ended.
        assert!(log.borrow().contains(&"recording flush".to_string()), "{:?}", log.borrow());
        assert!(!recorder.recording());

        // And nothing more is written afterwards.
        recorder.write_batch(&batch("Pressure")).unwrap();
        assert!(!log.borrow().contains(&"recording Pressure".to_string()), "{:?}", log.borrow());
    }

    #[test]
    fn starting_again_is_a_second_recording_not_more_of_the_first() {
        let log: Log = Rc::new(RefCell::new(Vec::new()));
        let armed = Arc::new(AtomicBool::new(false));
        let made = Rc::new(RefCell::new(0));
        let mut recorder = recorder(&armed, &log, &made);

        for _ in 0..2 {
            armed.store(true, Ordering::Relaxed);
            recorder.write_batch(&batch("Flow")).unwrap();
            armed.store(false, Ordering::Relaxed);
            recorder.flush().unwrap();
        }
        // A fresh sink each time, which is what gives CSV a new file and
        // SQLite a new run.
        assert_eq!(*made.borrow(), 2);
        assert_eq!(recorder.started(), 2);
    }

    #[test]
    fn a_start_is_noticed_even_when_no_data_is_arriving() {
        // Every device disconnected produces no batches at all. Pressing record
        // then must still start something, or it would look broken.
        let log: Log = Rc::new(RefCell::new(Vec::new()));
        let armed = Arc::new(AtomicBool::new(false));
        let made = Rc::new(RefCell::new(0));
        let mut recorder = recorder(&armed, &log, &made);

        armed.store(true, Ordering::Relaxed);
        recorder.flush().unwrap();
        assert!(recorder.recording());
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
