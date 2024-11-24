mod hardware;
mod error;
mod datapoint;
mod channel;
mod device;
mod daq;
// mod storage_hdf;
mod storage_csv;

pub use self::error::{Error, Result};
use hardware::mock_hardware;
use std::{thread, time};


fn main() -> Result<()> {
    let storage_path = std::path::PathBuf::from("test_results.csv");
    println!("Lets create some devices");
    let mut mock_device = mock_hardware::create_device("Test device".to_string());
    mock_hardware::add_channel_constant(&mut mock_device, "Constant 1".to_string())?;
    mock_hardware::add_channel_random(&mut mock_device, "Random 1".to_string())?;

    
    let mut daq = daq::Daq::new(
        vec![mock_device],
        storage_path,
    )?;

    // Setup storage
    daq.init_storage().unwrap_or_else(|err| panic!("Error intitialising storage: {err}"));
    // Connect to devices
    daq.connect();

    loop {
        // Read devices
        for dev in daq.devices.iter_mut() {
            dev.device_config.read(&mut dev.channels);
            dev.print_latest();
            match &mut daq.csv_writer {
                Some(wtr) => dev.write(wtr)?,
                None => ()
            }
        }
        // Wait
        thread::sleep(time::Duration::from_millis(500));
    }
}
