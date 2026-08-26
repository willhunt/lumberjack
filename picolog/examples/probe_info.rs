//! What every unit-info line actually contains, before and after a failure.
//!
//!     cargo run --example probe_info
//!
//! Error messages report a settings code and a driver code, and the driver code
//! keeps coming back as 8. The header only defines 0 to 5 for it, so either it
//! is being read wrong or it means something other than the header says.

use picolog::hrdl::{ Hrdl, Info, VoltageRange };

const LINES: [(&str, Info); 9] = [
    ("driver version", Info::DriverVersion),
    ("usb version", Info::UsbVersion),
    ("hardware version", Info::HardwareVersion),
    ("variant", Info::Variant),
    ("batch and serial", Info::BatchAndSerial),
    ("calibration date", Info::CalibrationDate),
    ("kernel driver", Info::KernelDriverVersion),
    ("HRDL_ERROR", Info::Error),
    ("HRDL_SETTINGS", Info::Settings),
];

fn main() {
    let mut unit = match Hrdl::open() {
        Ok(unit) => unit,
        Err(error) => {
            eprintln!("Could not open a unit: {}", error);
            std::process::exit(1);
        }
    };

    dump(&unit, "with nothing having gone wrong");

    // Provoke a known failure: differential on an even channel.
    let failed = unit.enable_channel(2, VoltageRange::MilliVolts2500, false);
    println!("\nafter a deliberate failure ({}):", match &failed {
        Ok(()) => "unexpectedly accepted".to_string(),
        Err(error) => error.to_string(),
    });
    dump(&unit, "");
}

fn dump(unit: &Hrdl, note: &str) {
    if !note.is_empty() {
        println!("{}:", note);
    }
    for (label, line) in LINES {
        match unit.info(line) {
            Ok(text) => println!("  {:<18} {:?}", label, text),
            Err(error) => println!("  {:<18} unavailable: {}", label, error),
        }
    }
}
