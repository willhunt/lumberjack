//! Talk to an attached ADC-20 or ADC-24 and report what happens.
//!
//!     cargo run --example probe -- [channel]
//!
//! Deliberately the smallest thing that exercises the whole path: load the
//! driver, open the unit, ask what it is, enable one channel and read it. If
//! this works the bindings and the wrapper are sound, and anything that fails
//! later is lumberdaq's side of the boundary rather than the FFI.

use picolog::hrdl::{ counts_to_volts, ConversionTime, Hrdl, Info, VoltageRange };
use std::time::Instant;

fn main() {
    let channel: u16 = std::env::args()
        .nth(1)
        .and_then(|argument| argument.parse().ok())
        .unwrap_or(1);

    let mut unit = match Hrdl::open() {
        Ok(unit) => unit,
        Err(error) => {
            eprintln!("Could not open a unit: {}", error);
            std::process::exit(1);
        }
    };
    println!("Opened a unit.\n");

    for (label, line) in [
        ("driver", Info::DriverVersion),
        ("hardware", Info::HardwareVersion),
        ("variant", Info::Variant),
        ("batch/serial", Info::BatchAndSerial),
        ("calibrated", Info::CalibrationDate),
    ] {
        match unit.info(line) {
            Ok(text) => println!("  {:<13} {}", label, text),
            Err(error) => println!("  {:<13} unavailable: {}", label, error),
        }
    }

    // 50Hz here. Wrong for a 60Hz supply, where mains hum would show up in the
    // readings rather than being rejected.
    if let Err(error) = unit.set_mains_rejection(false) {
        eprintln!("\nCould not set mains rejection: {}", error);
    }

    let range = VoltageRange::MilliVolts2500;
    let conversion = ConversionTime::Ms60;

    println!("\nChannel {} at +/-{} mV, {} ms conversion", channel, range.millivolts(), conversion.millis());

    if let Err(error) = unit.enable_channel(channel, range, true) {
        eprintln!("Could not enable channel {}: {}", channel, error);
        std::process::exit(1);
    }

    let (minimum, maximum) = match unit.count_range(channel) {
        Ok(counts) => counts,
        Err(error) => {
            eprintln!("Could not read the count range: {}", error);
            std::process::exit(1);
        }
    };
    println!("Counts span {} to {}\n", minimum, maximum);

    // Several reads, timed. The elapsed time should sit near the conversion
    // time, which is the number that decides how fast this device can be
    // sampled once it is a lumberdaq device.
    for reading_number in 1..=5 {
        let started = Instant::now();
        match unit.read_single(channel, range, conversion, true) {
            Ok(reading) => {
                let volts = counts_to_volts(reading.counts, maximum, range);
                println!(
                    "  {}  {:>10} counts  {:>10.6} V  {:>5.0} ms{}",
                    reading_number,
                    reading.counts,
                    volts,
                    started.elapsed().as_secs_f64() * 1000.0,
                    if reading.overflow { "  OVERFLOW" } else { "" }
                );
            }
            Err(error) => println!("  {}  failed: {}", reading_number, error),
        }
    }

    println!("\nClosing.");
    // Dropping the unit closes it. Nothing to call.
}
