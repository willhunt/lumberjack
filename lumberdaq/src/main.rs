use lumberdaq::configuration::write_configuration_file;
use lumberdaq::daq::Daq;
use lumberdaq::hardware::mock_hardware;
use lumberdaq::Result;
use std::{thread, time};

fn main() -> Result<()> {
    let storage_path = std::path::PathBuf::from("examples/simulated_devices/test_results.csv");
    let config_path = std::path::PathBuf::from("examples/simulated_devices/config.json");
    // Load config
    // read_configuration_file("examples/simulated_devices/config.json")?;

    println!("Lets create some devices");
    // Mock device
    let mut mock_hardware =
        mock_hardware::create_device("Test device".to_string(), "-".to_string())?;
    mock_hardware::add_channel_random(&mut mock_hardware, "Random 1".to_string())?;

    // TODO: Pico Technology / LabJack backends go here.

    // Serial device
    // let serial_port = serialport::SerialPortInfo {
    //     port_name: "COMx".to_string(),
    //     port_type: serialport::SerialPortType::Unknown
    // };

    let mut daq = Daq::new(
        "Example measurement".to_string(),
        "Joesephine Bloggs".to_string(),
        vec![mock_hardware],
        storage_path,
    )?;

    // Setup storage
    daq.init_storage()
        .unwrap_or_else(|err| panic!("Error intitialising storage: {err}"));
    // Connect to devices
    daq.connect()?;

    for _ in 0..10 {
        // Read devices
        for device in daq.devices.iter_mut() {
            device.read()?;
            device.print_latest();
            match &mut daq.csv_writer {
                Some(wtr) => device.write(wtr)?,
                None => (),
            }
        }
        // Wait
        thread::sleep(time::Duration::from_millis(200));
    }
    // TODO: Convert results here if required.
    write_configuration_file(&config_path, &daq)?;
    Ok(())
}
