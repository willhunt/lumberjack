mod hardware;
use hardware::mock_hardware;
use std::{thread, time};

mod datapoint;
mod channel;
mod device;
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
            dev.check_and_write();
        }
        // Wait
        thread::sleep(time::Duration::from_millis(500));
    }
}
