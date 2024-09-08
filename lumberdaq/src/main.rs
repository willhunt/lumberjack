mod hardware;
use hardware::{ mock_hardware, firmata };
use std::{thread, time};

mod devices;

fn main() {
    println!("Lets create some devices");

    let mut mock_device = mock_hardware::create_device();
    println!("Device created: {}", mock_device.name);

    let firmata_device = firmata::create_device();
    println!("Device created: {}", firmata_device.name);

    loop {
        // Read devices
        mock_hardware::read(&mut mock_device);
        mock_hardware::print_latest(&mock_device);
        // Wait
        thread::sleep(time::Duration::from_millis(1000));
    }
}
