mod hardware;
mod error;
mod datapoint;
mod channel;
mod device;
mod daq;
mod storage_hdf;
mod storage_csv;

pub use self::error::{Error, Result};
use hardware::mock_hardware;
use hardware::ni_usb6001;
use storage_hdf::convert_results_to_hdf5;
use std::{thread, time};


fn main() -> Result<()> {
    let storage_path = std::path::PathBuf::from("test_results.csv");
    println!("Lets create some devices");
    // Mock device
    let mut mock_device = mock_hardware::create_device("Test device".to_string());
    mock_hardware::add_channel_constant(&mut mock_device, "Constant 1".to_string())?;
    mock_hardware::add_channel_random(&mut mock_device, "Random 1".to_string())?;

    // National instruments
    let serial_port = serialport::SerialPortInfo {
        port_name: "COMx".to_string(),
        port_type: serialport::SerialPortType::Unknown
    };
    let mut usb6001_device = ni_usb6001::create_device("Virtual USB-6001".to_string(), serial_port);
    ni_usb6001::add_channel_analog(&mut usb6001_device, "Virtual sine wave".to_string(), "ai0".to_string())?;

    
    let mut daq = daq::Daq::new(
        "Example measurement".to_string(),
        "Joesephine Bloggs".to_string(),
        vec![mock_device, usb6001_device],
        storage_path,
    )?;

    // Setup storage
    daq.init_storage().unwrap_or_else(|err| panic!("Error intitialising storage: {err}"));
    // Connect to devices
    daq.connect();

    for _ in 0..10 {
        // Read devices
        for dev in daq.devices.iter_mut() {
            dev.config.read(&mut dev.channels);
            dev.print_latest();
            match &mut daq.csv_writer {
                Some(wtr) => dev.write(wtr)?,
                None => ()
            }
        }
        // Wait
        thread::sleep(time::Duration::from_millis(200));
    }
    convert_results_to_hdf5(&daq.csv_path)?;
    Ok(())
}
