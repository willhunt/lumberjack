//! Start and stop recording partway through a run.
//!
//!     cargo run --example record_in_bursts -- test_projects/scaled
//!
//! A run holds its sink for the whole run, so nothing can hand one over when
//! somebody presses record. A `Recorder` is attached from the start instead and
//! writes only while it is armed, which is how a display offers a record button
//! without the library needing to know a button exists.
//!
//! Here the flag is set by a thread on a timer rather than by a key press, but
//! it is the same flag and the same recorder.

use lumberdaq::config::StorageFormat;
use lumberdaq::daq::Daq;
use lumberdaq::project::Project;
use lumberdaq::storage::Recorder;
use std::sync::atomic::{ AtomicBool, Ordering };
use std::sync::Arc;
use std::thread;
use std::time::Duration;

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

fn run() -> lumberdaq::Result<()> {
    let directory = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "test_projects/scaled".to_string());

    let project = Project::new(&directory);
    let config = project.read_config()?;
    let storage = config.storage;
    let mut daq = Daq::from_config(config)?;

    let recording = Arc::new(AtomicBool::new(false));
    let sinks = Project::new(&directory);
    let mut burst = 0;
    daq.set_sink(Box::new(Recorder::new(
        Arc::clone(&recording),
        Box::new(move || {
            burst += 1;
            // Numbered rather than timestamped only so this prints something
            // predictable. A real caller would name it for the time.
            sinks.sink_for(storage, &format!("burst{}", burst))
        }),
    )))?;

    let report = daq.connect();
    if !report.all_connected() {
        for (device, reason) in report.failed.iter() {
            eprintln!("    {} did not connect: {}", device, reason);
        }
        return Ok(());
    }

    // Two bursts of recording inside one run, with the devices reading
    // throughout. What is not recorded is still being acquired and displayed.
    let flag = Arc::clone(&recording);
    thread::spawn(move || {
        for _ in 0..2 {
            thread::sleep(Duration::from_millis(300));
            flag.store(true, Ordering::Relaxed);
            thread::sleep(Duration::from_millis(700));
            flag.store(false, Ordering::Relaxed);
        }
    });

    println!("Watching for 3 seconds, recording two bursts of it.");
    daq.run_for(Duration::from_secs(3), &mut |_| {})?;

    println!("\nWhat is now in {}:", directory);
    let mut found: Vec<String> = std::fs::read_dir(&directory)
        .map_err(|error| format!("could not read {}: {}", directory, error))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .filter(|name| name.starts_with("results"))
        .collect();
    found.sort();
    for name in found.iter() {
        println!("    {}", name);
    }
    match storage {
        StorageFormat::Csv => println!("\nOne file per burst, since a CSV cannot say where one ends."),
        StorageFormat::Sqlite => println!("\nOne database; its runs table tells the bursts apart."),
    }
    Ok(())
}
