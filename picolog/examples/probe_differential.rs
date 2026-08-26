//! Find out what an attached unit really accepts for differential inputs.
//!
//!     cargo run --example probe_differential
//!
//! The documentation says a differential input pairs a channel with the one
//! above it, so only the odd numbered ones can be used. This asks the unit
//! rather than taking that on trust, and reports what the driver says about
//! each refusal.

use picolog::hrdl::{ counts_to_volts, ConversionTime, Hrdl, VoltageRange };

fn main() {
    let range = VoltageRange::MilliVolts2500;

    println!("Which channels accept each mode, asked one at a time:\n");
    println!("  {:<8} {:<28} {:<28}", "channel", "single ended", "differential");

    for channel in 1..=16u16 {
        let single = attempt(channel, range, true);
        let differential = attempt(channel, range, false);
        // Stop once neither works, which is past the end of this variant.
        if single.is_err() && differential.is_err() && channel > 8 {
            println!("  {:<8} {:<28} {:<28}", channel, describe(&single), describe(&differential));
            break;
        }
        println!("  {:<8} {:<28} {:<28}", channel, describe(&single), describe(&differential));
    }

    println!("\nCount range and a reading, where both modes work:\n");
    for channel in [1u16, 2] {
        for single_ended in [true, false] {
            let mode = if single_ended { "single ended" } else { "differential" };
            match reading(channel, range, single_ended) {
                Ok((minimum, maximum, volts)) => println!(
                    "  channel {} {:<13} counts {:>8} to {:<8}  reads {:>10.6} V",
                    channel, mode, minimum, maximum, volts
                ),
                Err(error) => println!("  channel {} {:<13} {}", channel, mode, error),
            }
        }
    }
}

fn attempt(
    channel: u16,
    range: VoltageRange,
    single_ended: bool,
) -> Result<(), picolog::hrdl::Error> {
    // A fresh unit each time, so one channel's settings cannot affect another's.
    let mut unit = Hrdl::open()?;
    unit.enable_channel(channel, range, single_ended)
}

fn reading(
    channel: u16,
    range: VoltageRange,
    single_ended: bool,
) -> Result<(i32, i32, f64), picolog::hrdl::Error> {
    let mut unit = Hrdl::open()?;
    unit.enable_channel(channel, range, single_ended)?;
    let (minimum, maximum) = unit.count_range(channel)?;
    let sample = unit.read_single(channel, range, ConversionTime::Ms60, single_ended)?;
    Ok((minimum, maximum, counts_to_volts(sample.counts, maximum, range)))
}

fn describe(result: &Result<(), picolog::hrdl::Error>) -> String {
    match result {
        Ok(()) => "accepted".to_string(),
        Err(error) => {
            let text = error.to_string();
            // Just the driver's complaint, not the whole sentence.
            match text.find(": ") {
                Some(position) => text[position + 2..].to_string(),
                None => text,
            }
        }
    }
}
