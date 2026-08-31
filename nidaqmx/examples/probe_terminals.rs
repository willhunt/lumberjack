//! Which channels really accept differential input, asked of the driver.
//!
//!     cargo run -p nidaqmx --example probe_terminals -- Dev1
//!
//! NI documents a differential input as pairing a channel with the one four
//! above it, so on an eight input device only the first four should be able to
//! start a pair. This asks rather than taking that on trust, and reports what
//! the driver says about each refusal.
//!
//! Worth asking because the equivalent probe on a Pico ADC-20 found the
//! documented rule and the real one were not quite the same thing, and because
//! a refusal that arrives on the first reading of a run is a wasted run.
//!
//! A configuration is tested by starting the task, not merely by adding the
//! channel: DAQmx checks some things only when a task is committed, so creating
//! a channel succeeding proves less than it appears to.

use nidaqmx::{ Daqmx, Task, Terminal };

/// What a USB-6001 spans. Asking for more than the hardware has would be
/// refused for the range rather than for the wiring, which is not the question.
const RANGE: (f64, f64) = (-10.0, 10.0);

fn main() {
    let device = std::env::args().nth(1).unwrap_or_else(|| "Dev1".to_string());

    let daqmx = match Daqmx::load() {
        Ok(daqmx) => daqmx,
        Err(error) => {
            eprintln!("{}", error);
            std::process::exit(1);
        }
    };

    let inputs = match daqmx.analog_inputs(&device) {
        Ok(inputs) => inputs,
        Err(error) => {
            eprintln!("Could not list the inputs of {}: {}", device, error);
            std::process::exit(1);
        }
    };

    match daqmx.serial_number(&device) {
        Ok(0) => println!(
            "{} is simulated. What it accepts is NI's model of the hardware,\n\
             not the hardware; ask the real unit before trusting it.\n",
            device
        ),
        _ => println!("{} is real hardware.\n", device),
    }

    println!("  {:<12} {:<30} {:<30}", "channel", "single ended", "differential");
    for channel in inputs.iter() {
        let single = attempt(&daqmx, channel, Terminal::SingleEnded);
        let differential = attempt(&daqmx, channel, Terminal::Differential);
        println!(
            "  {:<12} {:<30} {:<30}",
            channel,
            describe(&single),
            describe(&differential)
        );
    }

    // One refusal in full. The table has to cut them short, and what the
    // driver actually says is the useful part: it lists what it would accept.
    if let Some(refusal) = inputs
        .iter()
        .find_map(|channel| attempt(&daqmx, channel, Terminal::Differential).err())
    {
        println!("\nWhat a refusal says in full:\n");
        for line in refusal.to_string().lines() {
            println!("  {}", line);
        }
    }

    println!("\nA reading from the first input, both ways:\n");
    if let Some(channel) = inputs.first() {
        for terminal in [Terminal::SingleEnded, Terminal::Differential] {
            match reading(&daqmx, channel, terminal) {
                Ok(volts) => println!("  {:<14?} {:>12.6} V", terminal, volts),
                Err(error) => println!("  {:<14?} {}", terminal, error),
            }
        }
    }
}

/// Configure one channel one way, and commit it.
///
/// A task of its own each time, so that one channel's settings cannot affect
/// what the next one is allowed to do.
fn attempt(daqmx: &Daqmx, channel: &str, terminal: Terminal) -> nidaqmx::Result<()> {
    let mut task = configured(daqmx, channel, terminal)?;
    task.start()
}

fn reading(daqmx: &Daqmx, channel: &str, terminal: Terminal) -> nidaqmx::Result<f64> {
    let mut task = configured(daqmx, channel, terminal)?;
    let values = task.read_one()?;
    Ok(values.first().copied().unwrap_or(f64::NAN))
}

fn configured(daqmx: &Daqmx, channel: &str, terminal: Terminal) -> nidaqmx::Result<Task> {
    let mut task = daqmx.task("")?;
    task.add_voltage_input(channel, terminal, RANGE)?;
    Ok(task)
}

fn describe(outcome: &nidaqmx::Result<()>) -> String {
    match outcome {
        Ok(()) => "accepted".to_string(),
        Err(error) => {
            // Just the driver's complaint, and only its first line: DAQmx
            // explains itself at length, and the rest is advice about MAX.
            let text = error.to_string();
            let complaint = match text.find(": ") {
                Some(position) => &text[position + 2..],
                None => text.as_str(),
            };
            let first = complaint.lines().next().unwrap_or("refused");
            match first.len() > 28 {
                true => format!("{}...", &first[..25]),
                false => first.to_string(),
            }
        }
    }
}
