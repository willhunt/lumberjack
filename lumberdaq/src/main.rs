mod hardware;
use hardware::{ mock_hardware, firmata };
use std::{thread, time};

mod devices;

mod daq;

fn main() {
    println!("Lets create some devices");
    // let firmata_device = firmata::create_device();
    // println!("Device created: {}", firmata_device.name);

    let mut daq = daq::Daq{
        devices: vec![
            Box::new(mock_hardware::MockDevice::new())
        ]
    };

    loop {
        // Read devices
        for mut device in daq.devices.iter_mut() {
            device.read();
            // device.device.print_latest();
            println!("Device read: {}", device.device.name);
        }
        // Wait
        thread::sleep(time::Duration::from_millis(1000));
    }
}
