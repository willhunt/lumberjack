use lumberdaq::Result;
use std::time::Duration;
use lumberdaq::daq::Daq;
use lumberdaq::project::Project;
use lumberdaq::session::DeviceEvent;
use lumberdaq::storage_csv::CsvSink;

fn main() -> Result<()> {
    // Re-run an existing project: read the setup that was saved in this
    // directory and rebuild it. Nothing here needs to know any file paths,
    // only where the project lives.
    let project = Project::new("examples/simulated_devices");
    let config = project.read_config()?;
    let mut daq = Daq::from_config(config)?;

    // Setup storage
    let sink = CsvSink::new(&project.results_path(), &project.header_path())?;
    daq.set_sink(Box::new(sink))
        .unwrap_or_else(|err| panic!("Error intitialising storage: {err}"));

    // Connect to devices
    let report = daq.connect();
    for (name, reason) in report.failed.iter() {
        eprintln!("Could not connect to {}: {}", name, reason);
    }

    // Record for a fixed time. Each device keeps its own sample interval from
    // the config, on its own thread.
    daq.run_for(Duration::from_secs(10), &mut |event| {
        if let DeviceEvent::Problem { device, error } = event {
            eprintln!("    ! {}: {}", device, error);
        }
    })?;

    for device in daq.devices.iter() {
        device.print_latest();
    }
    Ok(())
}
