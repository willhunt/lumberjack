use lumberdaq::Result;
use std::{thread, time};
use lumberdaq::configuration::read_configuration_file;

fn main() -> Result<()> {
    let storage_path = std::path::PathBuf::from("examples/simulated_devices/test_results_2.csv");
    let config_path = std::path::PathBuf::from("examples/simulated_devices/config.json");
    // Load config
    let mut daq = read_configuration_file(&config_path)?;
    daq.csv_path = storage_path;

    // Setup storage
    daq.init_storage().unwrap_or_else(|err| panic!("Error intitialising storage: {err}"));
    // Connect to devices
    daq.connect()?;

    for _ in 0..10 {
        // Read devices
        for device in daq.devices.iter_mut() {
            device.read()?;
            device.print_latest();
            match &mut daq.csv_writer {
                Some(wtr) => device.write(wtr)?,
                None => ()
            }
        }
        // Wait
        thread::sleep(time::Duration::from_millis(200));
    }
    Ok(())
}
