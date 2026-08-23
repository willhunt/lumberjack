//! Record a project from the command line.
//!
//!     lumberdaq [project directory]
//!
//! The directory holds config.json describing the devices, and is where the
//! results are written. Defaults to the current directory, so running inside a
//! project needs no argument at all.

use lumberdaq::session::DeviceEvent;
use lumberdaq::Result;
use std::sync::atomic::{ AtomicBool, Ordering };
use std::sync::Arc;

const HELP: &str = "\
Record a data acquisition project.

USAGE:
    lumberdaq [PROJECT]

ARGS:
    PROJECT    Directory holding config.json. Defaults to the current directory.

Recording continues until interrupted with Ctrl-C.
";

fn main() -> Result<()> {
    let argument = std::env::args().nth(1);
    if let Some(argument) = argument.as_deref() {
        if argument == "-h" || argument == "--help" {
            print!("{}", HELP);
            return Ok(());
        }
    }
    let directory = argument.unwrap_or_else(|| ".".to_string());

    // Everything about the setup, including which format to record in, comes
    // from the directory. Nothing here needs recompiling to change a channel.
    let mut daq = lumberdaq::open(&directory)?;
    println!("Project: {}", directory);

    // Try every device, so one bad port does not hide the state of the rest.
    let report = daq.connect();
    for name in report.connected.iter() {
        println!("    connected  {}", name);
    }
    if !report.all_connected() {
        eprintln!();
        eprintln!("Could not connect to {} of {} devices:", report.failed.len(), daq.devices.len());
        for (name, reason) in report.failed.iter() {
            eprintln!("    {}: {}", name, reason);
        }
        // Recording a run that is quietly missing a device wastes the run.
        return Err("Not all devices connected. Fix the above, or remove those devices from the setup.".into());
    }

    // Ctrl-C sets the flag rather than killing the process, so the run stops
    // tidily and the sink gets its final flush. Killing it outright would lose
    // whatever had not reached disk.
    let stop = Arc::new(AtomicBool::new(false));
    let handler_flag = Arc::clone(&stop);
    ctrlc::set_handler(move || handler_flag.store(true, Ordering::Relaxed))
        .map_err(|error| format!("Could not listen for Ctrl-C: {}", error))?;

    println!("\nRecording. Press Ctrl-C to stop.\n");
    daq.run(&stop, &mut |event| match event {
        DeviceEvent::Problem { device, error } => {
            eprintln!("    ! {}: {}", device, error);
        }
        DeviceEvent::Connected { device } => {
            println!("    reconnected  {}", device);
        }
        DeviceEvent::Disconnected { device, cause } => {
            eprintln!(
                "    ! lost {}: {}",
                device,
                cause.unwrap_or_else(|| "unknown".to_string())
            );
        }
    })?;

    println!("\nStopped.");
    for device in daq.devices.iter() {
        device.print_latest();
    }
    Ok(())
}
