//! What the driver says is attached, before anything is read from it.
//!
//!     cargo run -p nidaqmx --example probe_devices
//!
//! The first thing worth knowing is whether the driver loads at all, and the
//! second is what it calls the hardware, since DAQmx addresses channels by
//! device name and a config file has to hold the right one.
//!
//! A simulated device made in NI MAX appears here exactly as a real one does,
//! which is what makes it useful to develop against.

fn main() {
    let daqmx = match nidaqmx::Daqmx::load() {
        Ok(daqmx) => daqmx,
        Err(error) => {
            eprintln!("{}", error);
            std::process::exit(1);
        }
    };
    println!("Driver loaded.\n");

    let devices = match daqmx.devices() {
        Ok(devices) => devices,
        Err(error) => {
            eprintln!("Could not list devices: {}", error);
            std::process::exit(1);
        }
    };

    if devices.is_empty() {
        println!("The driver is there but knows of no devices.");
        println!("Plug one in, or make a simulated one in NI MAX.");
        return;
    }

    for device in devices.iter() {
        let model = daqmx
            .product_type(device)
            .unwrap_or_else(|error| format!("unknown ({})", error));
        // Zero means simulated, which is worth saying outright: a simulated
        // device will accept settings the hardware might not.
        let serial = match daqmx.serial_number(device) {
            Ok(0) => "simulated".to_string(),
            Ok(serial) => format!("serial {:X}", serial),
            Err(error) => format!("serial unavailable ({})", error),
        };
        println!("  {}  {}  {}", device, model, serial);

        match daqmx.analog_inputs(device) {
            Ok(inputs) if inputs.is_empty() => println!("      no analog inputs"),
            Ok(inputs) => {
                println!("      {} analog inputs", inputs.len());
                for input in inputs.iter() {
                    println!("          {}", input);
                }
            }
            Err(error) => println!("      could not list analog inputs: {}", error),
        }
    }
}
