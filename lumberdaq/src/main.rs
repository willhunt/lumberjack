mod hardware;
use hardware::mock_hardware;
use std::{thread, time};

mod datapoint;
mod channel;
mod device;
mod daq;
mod storage;

fn main() {
    let storage_path = std::path::PathBuf::from("test_results.hdf5");
    println!("Lets create some devices");
    let mut mock_device = mock_hardware::create_device("Test device".to_string());
    mock_hardware::add_channel_constant(&mut mock_device, "Constant 1".to_string());
    mock_hardware::add_channel_random(&mut mock_device, "Random 1".to_string());

    
    let mut daq = daq::Daq::new(
        vec![mock_device],
        storage_path,
    );

    // Setup storage
    daq.init_storage().unwrap_or_else(|err| panic!("Error intitialising storage: {err}"));
    // Connect to devices
    daq.connect();

    loop {
        // Read devices
        for dev in daq.devices.iter_mut() {
            dev.device_config.read(&mut dev.channels);
            dev.print_latest();
            dev.check_and_write();
        }
        // Wait
        thread::sleep(time::Duration::from_millis(500));
    }
}
