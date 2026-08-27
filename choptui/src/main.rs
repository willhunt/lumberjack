//! Watch a lumberdaq acquisition in a terminal.
//!
//!     choptui [project directory]
//!
//! Reads the same project directory the CLI records from. Nothing is recorded
//! yet: this connects, shows what the devices are reading, and stops when told
//! to. Recording arrives with the button in the header.

mod monitor;
mod plot_config;
mod ui;

use lumberdaq::daq::Daq;
use lumberdaq::project::Project;
use lumberdaq::storage::{ Fanout, Recorder };
use monitor::{ Monitor, Update };
use std::sync::atomic::{ AtomicBool, Ordering };
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

fn main() {
    if let Err(error) = run() {
        // Display and not Debug, or every message the library takes care to
        // write comes out as a struct dump.
        eprintln!("Error: {}", error);
        let mut cause = std::error::Error::source(&error);
        while let Some(next) = cause {
            eprintln!("  caused by: {}", next);
            cause = next.source();
        }
        // The library says a project is not there; only this knows what to
        // type instead.
        if matches!(error, lumberdaq::Error::NoProjectHere { .. }) {
            eprintln!("\nUSAGE: choptui [PROJECT]");
        }
        std::process::exit(1);
    }
}

fn run() -> lumberdaq::Result<()> {
    let directory = std::env::args().nth(1).unwrap_or_else(|| ".".to_string());

    // Deliberately not `lumberdaq::open`, which attaches the project's sink and
    // so creates the results file before anything has been recorded. Watching a
    // rig should leave nothing behind.
    let config = Project::new(&directory).read_config()?;
    let mut state = ui::State::from_config(&directory, &config);

    // A layout saved last time, if there is one. Never a reason not to open:
    // a plot naming a channel this setup does not have is said so in the log
    // and skipped, since the alternative is refusing to show a rig because of
    // something that only affects how it is drawn.
    match plot_config::read(&directory) {
        Ok(Some(saved)) => state.apply_plot_config(saved),
        Ok(None) => {}
        Err(problem) => state.note(problem),
    }
    // Read before the config is handed over, so the recorder knows what format
    // to write when somebody eventually asks it to.
    let storage = config.storage;
    let mut daq = Daq::from_config(config)?;

    let (updates, from_run) = mpsc::channel::<Update>();

    // Set by the display when record is pressed, read by the recorder on the
    // collecting thread. A flag rather than a message because there is nothing
    // to queue: what matters is whether it is on now.
    let recording = Arc::new(AtomicBool::new(false));
    let sinks = Project::new(&directory);
    daq.set_sink(Box::new(
        Fanout::new()
            .and("display", Box::new(Monitor::new(updates.clone())))
            .and(
                "recording",
                Box::new(Recorder::new(
                    Arc::clone(&recording),
                    // Named for when it started. A database ignores the label
                    // and keeps its runs in the one file; a CSV gets a file of
                    // its own each time, which is what stopping and starting
                    // has to mean when a format cannot say where one recording
                    // ends and the next begins.
                    Box::new(move || {
                        let label = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
                        sinks.sink_for(storage, &label)
                    }),
                )),
            ),
    ))?;

    // Connecting before the display starts, so the first frame already shows
    // which devices came up instead of a screen of unknowns.
    let report = daq.connect();
    for device in report.connected.iter() {
        let _ = updates.send(Update::Connected { device: device.clone() });
    }
    for (device, reason) in report.failed.iter() {
        let _ = updates.send(Update::Disconnected {
            device: device.clone(),
            cause: Some(reason.to_string()),
        });
    }

    // The display asks for the run to end by setting this, exactly as Ctrl-C
    // would. `run` blocks until then, so the drawing has to be the other
    // thread rather than this one.
    let stop = Arc::new(AtomicBool::new(false));
    let display = {
        let stop = Arc::clone(&stop);
        let recording = Arc::clone(&recording);
        thread::spawn(move || ui::run(from_run, state, &stop, &recording))
    };

    let outcome = daq.run(&stop, &mut |event| {
        // Ignored for the same reason the sink ignores it: the display going
        // away must not take the run with it.
        let _ = updates.send(Update::from_event(event));
    });

    // Whether the run ended by request or by failing, the display is finished.
    stop.store(true, Ordering::Relaxed);
    match display.join() {
        Ok(Ok(())) => {}
        // The terminal is already restored by then, so this is safe to print.
        Ok(Err(error)) => eprintln!("Display error: {}", error),
        Err(_) => eprintln!("The display panicked."),
    }
    outcome
}
