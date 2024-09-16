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
        for dev in daq.devices.iter_mut() {
            dev.read();
            dev.print_latest();
        }
        // Wait
        thread::sleep(time::Duration::from_millis(1000));
    }
}
