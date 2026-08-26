//! Watch a lumberdaq acquisition in a terminal.
//!
//!     choptui [project directory]
//!
//! Reads the same project directory the CLI records from. Nothing is recorded
//! yet: this connects, shows what the devices are reading, and stops when told
//! to. Recording arrives with the button in the header.

mod monitor;
mod ui;

use lumberdaq::daq::Daq;
use lumberdaq::project::Project;
use lumberdaq::storage::Fanout;
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
    let state = ui::State::from_config(&directory, &config);
    let mut daq = Daq::from_config(config)?;

    let (updates, from_run) = mpsc::channel::<Update>();
    daq.set_sink(Box::new(
        Fanout::new().and("display", Box::new(Monitor::new(updates.clone()))),
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
        thread::spawn(move || ui::run(from_run, state, &stop))
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
