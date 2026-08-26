//! Record one run to CSV and SQLite at once.
//!
//!     cargo run --example two_sinks -- test_projects/scaled
//!
//! A run has one sink. Recording to two places is therefore a sink that is
//! itself two, which is all `Fanout` is. Nothing in `Daq` knows the difference.
//!
//! Worth doing for real when a run matters: SQLite to query afterwards, CSV to
//! hand to someone who only has a spreadsheet.

use lumberdaq::config::StorageFormat;
use lumberdaq::project::Project;
use lumberdaq::storage::Fanout;
use std::time::Duration;

fn main() {
    if let Err(error) = run() {
        eprintln!("Error: {}", error);
        // Whatever the sink said underneath, rather than only the summary.
        let mut cause = std::error::Error::source(&error);
        while let Some(error) = cause {
            eprintln!("  caused by: {}", error);
            cause = std::error::Error::source(error);
        }
        std::process::exit(1);
    }
}

fn run() -> lumberdaq::Result<()> {
    let directory = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "test_projects/scaled".to_string());

    let project = Project::new(&directory);
    let mut daq = lumberdaq::open(&directory)?;

    // The project decides where each format writes, so both land beside the
    // config rather than wherever this was run from.
    daq.set_sink(Box::new(
        Fanout::new()
            .and("sqlite", project.sink(StorageFormat::Sqlite)?)
            .and("csv", project.sink(StorageFormat::Csv)?),
    ))?;

    let report = daq.connect();
    if !report.all_connected() {
        for (name, reason) in report.failed.iter() {
            eprintln!("    {} did not connect: {}", name, reason);
        }
        return Ok(());
    }

    println!("Recording {} to both sinks for two seconds.", directory);
    daq.run_for(Duration::from_secs(2), &mut |event| {
        if let lumberdaq::session::DeviceEvent::Problem { device, error } = event {
            eprintln!("    ! {}: {}", device, error);
        }
    })?;

    println!("Done. Both files are in {}.", directory);
    Ok(())
}
