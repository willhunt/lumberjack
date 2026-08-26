//! Terminal monitor for a lumberdaq run.
//!
//! Nothing is drawn yet. This exists to hold the wiring: because choptui is a
//! separate crate rather than a module of lumberdaq, anything it needs has to
//! be public API, and a missing export shows up as a compile error here instead
//! of as a surprise when a user interface is built on the same surface.
//!
//!     cargo run -p choptui -- lumberdaq/test_projects/scaled

use lumberdaq::project::Project;

fn main() {
    if let Err(error) = run() {
        // Printed with Display and not Debug, or every message the library
        // takes care to write would come out as a struct dump.
        eprintln!("Error: {}", error);
        std::process::exit(1);
    }
}

fn run() -> lumberdaq::Result<()> {
    let directory = std::env::args().nth(1).unwrap_or_else(|| ".".to_string());
    let config = Project::new(&directory).read_config()?;

    println!("{} by {}", config.info.name, config.info.author);
    for input in config.available_inputs().iter() {
        println!("    {}", input);
    }
    Ok(())
}
