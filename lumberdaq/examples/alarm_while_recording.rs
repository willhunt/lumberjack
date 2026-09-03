//! Watch a channel for a limit while the run is being recorded.
//!
//!     cargo run --example alarm_while_recording -- test_projects/scaled Pressure 8
//!
//! A run has one sink, so doing two things with the data means a sink that is
//! itself two. That is all `Fanout` is, and this is the pairing it exists for:
//! one sink writing to disk, and one doing something else entirely.
//!
//! The something else here is an alarm, which writes nothing at all. It is the
//! same shape as the display in choptui, which is a sink alongside the file it
//! is being recorded to, and it is how anything else would attach — a live
//! plot, a network feed, a valve that ought to close.
//!
//! Because it sees the batches the recorder sees, it cannot disagree with what
//! was written. An alarm fed by a second read of the hardware could say the
//! pressure never went over while the results file said it did.

use lumberdaq::config::DaqConfig;
use lumberdaq::daq::Daq;
use lumberdaq::project::Project;
use lumberdaq::storage::{ Batch, DataSink, Fanout };
use lumberdaq::Result;
use std::time::Duration;

/// Says when a channel goes over a limit, and when it comes back under.
///
/// This is the whole of implementing a sink: three methods, none of which has
/// to write anything anywhere.
struct Alarm {
    channel: String,
    limit: f64,
    /// Whether it is currently over. Without this every reading above the limit
    /// would be reported, which for a channel sitting just over it is a page of
    /// the same news.
    over: bool,
    crossings: usize,
}

impl DataSink for Alarm {
    /// Called once, with the whole setup, before any data.
    ///
    /// Nothing to do here: this alarm is told which channel to watch when it is
    /// built. A sink that wanted units, or to check the channel exists at all,
    /// would read them from the config it is handed.
    fn init(&mut self, _config: &DaqConfig) -> Result<()> {
        Ok(())
    }

    /// One channel's readings, as they are acquired.
    ///
    /// Every sink is offered every batch, so a sink that cares about one
    /// channel says so itself. There is no subscribing.
    fn write_batch(&mut self, batch: &Batch) -> Result<()> {
        if batch.channel != self.channel {
            return Ok(());
        }
        for point in batch.datapoints.iter() {
            // Each reading in turn rather than only the newest. A batch holds
            // everything acquired since the last drain, and an excursion that
            // began and ended inside one of them is exactly what an alarm is
            // for.
            match (point.value > self.limit, self.over) {
                (true, false) => {
                    self.over = true;
                    self.crossings += 1;
                    println!(
                        "  {}  {} over {} at {:.3}  (crossing {})",
                        point.datetime.format("%H:%M:%S%.3f"),
                        self.channel,
                        self.limit,
                        point.value,
                        self.crossings
                    );
                }
                (false, true) => {
                    self.over = false;
                    println!(
                        "  {}  {} back under at {:.3}",
                        point.datetime.format("%H:%M:%S%.3f"),
                        self.channel,
                        point.value
                    );
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Push anything buffered out. Nothing is buffered, so nothing to push.
    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("Error: {}", error);
        let mut cause = std::error::Error::source(&error);
        while let Some(next) = cause {
            eprintln!("  caused by: {}", next);
            cause = next.source();
        }
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let mut arguments = std::env::args().skip(1);
    let directory = arguments.next().unwrap_or_else(|| "test_projects/scaled".to_string());
    let channel = arguments.next().unwrap_or_else(|| "Pressure".to_string());
    let limit: f64 = arguments.next().and_then(|text| text.parse().ok()).unwrap_or(8.0);

    // Deliberately not `lumberdaq::open`, which attaches the project's sink for
    // us. There is one to attach here and it is not that one.
    let project = Project::new(&directory);
    let config = project.read_config()?;
    let storage = config.storage;
    let mut daq = Daq::from_config(config)?;

    daq.set_sink(Box::new(
        Fanout::new()
            .and("database", project.sink(storage)?)
            .and(
                "alarm",
                Box::new(Alarm {
                    channel: channel.clone(),
                    limit: limit,
                    over: false,
                    crossings: 0,
                }),
            ),
    ))?;

    let report = daq.connect();
    if !report.all_connected() {
        for (name, reason) in report.failed.iter() {
            eprintln!("    {} did not connect: {}", name, reason);
        }
        return Ok(());
    }

    println!("Recording {}, watching {} for {}.\n", directory, channel, limit);
    daq.run_for(Duration::from_secs(6), &mut |event| {
        if let lumberdaq::session::DeviceEvent::Problem { device, error } = event {
            eprintln!("    ! {}: {}", device, error);
        }
    })?;

    // The alarm went into the Fanout, and the Fanout into the Daq, so it cannot
    // be asked afterwards how many times it fired. A caller wanting that back
    // would share a counter with it, the way choptui shares its recording flag.
    println!("\nDone. Everything above was noticed while it was being recorded,");
    println!("by the same batches that went into {}.", project.database_path().display());
    Ok(())
}
