mod hardware;
mod error;
mod datapoint;
mod channel;
mod device;
mod daq;
mod storage_hdf;
mod storage_csv;
mod configuration;

pub use self::error::{Error, Result};
use hardware::{ mock_hardware, ni_usb6001, Hardware };
use storage_hdf::convert_results_to_hdf5;
use std::{thread, time};
// use configuration::read_configuration_file;


fn main() -> Result<()> {
    let storage_path = std::path::PathBuf::from("test_results.csv");
    let config_path = std::path::PathBuf::from("examples/simulated_devices/config.json");
    // Load config
    // read_configuration_file("examples/simulated_devices/config.json")?;

    println!("Lets create some devices");
    // Mock device
    let mut mock_hardware = mock_hardware::create_device("Test device".to_string(), "-".to_string())?;
    mock_hardware::add_channel_random(&mut mock_hardware, "Random 1".to_string())?;

    // National instruments
    let mut usb6001 = ni_usb6001::create_device("Virtual USB-6001".to_string(), "NIUSB-6001".to_string())?;
    ni_usb6001::add_channel_analog(&mut usb6001, "Virtual signal 0".to_string(), "NIUSB-6001/ai0".to_string())?;
    ni_usb6001::add_channel_analog(&mut usb6001, "Virtual signal 1".to_string(), "NIUSB-6001/ai1".to_string())?;

    // Serial device
     // let serial_port = serialport::SerialPortInfo {
    //     port_name: "COMx".to_string(),
    //     port_type: serialport::SerialPortType::Unknown
    // };
    
    let mut daq = daq::Daq::new(
        "Example measurement".to_string(),
        "Joesephine Bloggs".to_string(),
        vec![mock_hardware, usb6001],
        storage_path,
    )?;

    // Setup storage
    daq.init_storage().unwrap_or_else(|err| panic!("Error intitialising storage: {err}"));
    // Connect to devices
    daq.connect();

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
    convert_results_to_hdf5(&daq.csv_path)?;
    Ok(())
}
