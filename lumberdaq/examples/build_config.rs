//! Write a config.json describing a test rig.
//!
//!     cargo run --example build_config -- test_projects/simulated_devices
//!
//! Defining devices in Rust is a convenience for producing something to test
//! against, not how a setup is normally made: a config is data, and once this
//! has written one the project runs without recompiling anything.
//!
//!     cargo run -- test_projects/simulated_devices

use lumberdaq::config::StorageFormat;
use lumberdaq::daq::Daq;
use lumberdaq::hardware::{ mock_hardware, serial_stream };
use lumberdaq::project::Project;
use lumberdaq::Result;
use std::time::Duration;

fn main() -> Result<()> {
    let directory = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "test_projects/simulated_devices".to_string());
    let project = Project::create(&directory)?;

    // A device with no hardware behind it, for running with nothing attached.
    let mut mock = mock_hardware::create_device("Test device".to_string(), "-".to_string())?;
    mock_hardware::add_channel_random(&mut mock, "Random 1".to_string())?;

    // A device streaming comma separated frames over serial.
    let mut serial = serial_stream::create_device(
        "Serial test device".to_string(),
        "Device streaming over serial.".to_string(),
        "COM3".to_string(),
        115200,
    )?;
    serial_stream::add_channel(
        &mut serial,
        "Pressure".to_string(),
        "Differential pressure sensor".to_string(),
        1,
        "Pa".to_string(),
    )?;
    serial_stream::add_channel(
        &mut serial,
        "Pump Activation".to_string(),
        "Pump on (1) or off (0)".to_string(),
        3,
        "-".to_string(),
    )?;

    // Deliberately different rates, since each device reads on its own thread.
    mock.read_interval = Duration::from_millis(500);
    serial.read_interval = Duration::from_millis(100);

    let daq = Daq::new(
        "Example measurement".to_string(),
        "Joesephine Bloggs".to_string(),
        vec![mock, serial],
    )?;

    let mut config = daq.config();
    config.storage = StorageFormat::Sqlite;
    project.write_config(&config)?;

    println!("Wrote {}", project.config_path().display());
    println!("Run it with:  cargo run -- {}", directory);
    Ok(())
}
