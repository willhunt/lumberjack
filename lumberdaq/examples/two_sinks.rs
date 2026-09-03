//! Record one run to two places at once.
//!
//!     cargo run --example two_sinks -- test_projects/scaled
//!
//! A run has one sink. Recording to two places is therefore a sink that is
//! itself two, which is all `Fanout` is. Nothing in `Daq` knows the difference.
//!
//! Here that is the project's own database and a second copy beside it, which
//! is worth doing when a run matters and the disk it is going to might not come
//! back: a rig in a test cell writing to a network share as well as to itself.
//! It is also how a live display attaches, which is what choptui does — a
//! display is a sink alongside the file being written.
//!
//! Every sink is offered each batch even if an earlier one fails, so the copy
//! that is still working cannot be starved by the one that is not.

use lumberdaq::config::StorageFormat;
use lumberdaq::project::Project;
use lumberdaq::storage::Fanout;
use lumberdaq::storage_sqlite::SqliteSink;
use std::time::Duration;

fn main() {
    if let Err(error) = run() {
        eprintln!("Error: {}", error);
        // Whatever the sink said underneath, rather than only the summary.
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
    let mut daq = lumberdaq::open(&directory)?;

    // Where a second copy would go. A different disk in earnest; next to the
    // first one here, so the example needs nothing set up.
    let spare = project.export_path().with_file_name("results-copy.db");

    daq.set_sink(Box::new(
        Fanout::new()
            .and("database", project.sink(StorageFormat::Sqlite)?)
            .and("copy", Box::new(SqliteSink::new(&spare)?)),
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

    println!("Done.");
    println!("    {}", project.database_path().display());
    println!("    {}", spare.display());
    Ok(())
}
