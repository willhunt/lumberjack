//! Stream from an attached ADC-20 or ADC-24 and report what arrives.
//!
//!     cargo run --example probe_stream -- [channels] [interval_ms]
//!
//! The counterpart to `probe`, which asks for one value at a time. This starts
//! the unit scanning on its own schedule and drains what it produced, so the
//! things to watch are that no scans are missed between drains, and that the
//! unit's own timestamps step evenly regardless of when we happened to ask.

use picolog::hrdl::{ counts_to_volts, ConversionTime, Hrdl, VoltageRange };
use std::thread;
use std::time::{ Duration, Instant };

fn main() {
    let channel_count: u16 = std::env::args()
        .nth(1)
        .and_then(|argument| argument.parse().ok())
        .unwrap_or(2);
    let interval_ms: u64 = std::env::args()
        .nth(2)
        .and_then(|argument| argument.parse().ok())
        .unwrap_or(200);

    let range = VoltageRange::MilliVolts2500;
    let conversion = ConversionTime::Ms60;

    let mut unit = match Hrdl::open() {
        Ok(unit) => unit,
        Err(error) => {
            eprintln!("Could not open a unit: {}", error);
            std::process::exit(1);
        }
    };

    let mut full_scale = Vec::new();
    for channel in 1..=channel_count {
        if let Err(error) = unit.enable_channel(channel, range, true) {
            eprintln!("Could not enable channel {}: {}", channel, error);
            std::process::exit(1);
        }
        match unit.count_range(channel) {
            Ok((_minimum, maximum)) => full_scale.push(maximum),
            Err(error) => {
                eprintln!("Could not read the count range: {}", error);
                std::process::exit(1);
            }
        }
    }

    let interval = Duration::from_millis(interval_ms);
    if let Err(error) = unit.set_interval(interval, conversion) {
        eprintln!(
            "Could not set a {} ms interval for {} channels at {} ms conversion: {}",
            interval_ms,
            channel_count,
            conversion.millis(),
            error
        );
        eprintln!("The unit needs an interval long enough for every channel to convert.");
        std::process::exit(1);
    }

    // Room for plenty more than one drain, so a slow reader does not lose scans.
    if let Err(error) = unit.start_streaming(1000) {
        eprintln!("Could not start streaming: {}", error);
        std::process::exit(1);
    }

    println!(
        "Streaming {} channels every {} ms, {} ms conversion.\n",
        channel_count, interval_ms, conversion.millis()
    );

    // Deliberately drain on an uneven schedule. The unit's timestamps should
    // still step by the interval, which is the whole point of using them
    // rather than stamping when we happened to look.
    let drain_waits = [300u64, 700, 250, 1200, 400];
    let started = Instant::now();
    let mut total = 0usize;
    let mut previous_scan_time: Option<Duration> = None;
    let mut first_scan_time: Option<Duration> = None;
    let mut last_scan_time = Duration::ZERO;

    for wait in drain_waits {
        thread::sleep(Duration::from_millis(wait));
        let scans = match unit.take_scans(channel_count as usize, 512) {
            Ok(scans) => scans,
            Err(error) => {
                eprintln!("Drain failed: {}", error);
                break;
            }
        };
        total += scans.len();
        println!(
            "  after {:>5} ms idle: {} scans (wall clock {:.0} ms)",
            wait,
            scans.len(),
            started.elapsed().as_secs_f64() * 1000.0
        );
        for scan in scans.iter() {
            let step = match previous_scan_time {
                Some(previous) => format!("{:>6.0} ms", (scan.since_start - previous).as_secs_f64() * 1000.0),
                None => "     -".to_string(),
            };
            previous_scan_time = Some(scan.since_start);
            first_scan_time.get_or_insert(scan.since_start);
            last_scan_time = scan.since_start;
            let volts: Vec<String> = scan
                .counts
                .iter()
                .zip(full_scale.iter())
                .map(|(counts, maximum)| format!("{:>10.6} V", counts_to_volts(*counts, *maximum, range)))
                .collect();
            println!(
                "      unit t={:>7.0} ms  step {}  {}{}",
                scan.since_start.as_secs_f64() * 1000.0,
                step,
                volts.join("  "),
                if scan.overflow { "  OVERFLOW" } else { "" }
            );
        }
    }

    unit.stop();

    // Rate measured on the unit's clock, not ours. Dividing by our wall clock
    // folds in the time before the first scan and after the last, and reports a
    // rate the unit never ran at.
    let span = (last_scan_time - first_scan_time.unwrap_or(Duration::ZERO)).as_secs_f64();
    let rate = if span > 0.0 { (total - 1) as f64 / span } else { 0.0 };
    println!(
        "\n{} scans spanning {:.1} s of unit time = {:.2} Hz (asked for {:.2} Hz)",
        total,
        span,
        rate,
        1000.0 / interval_ms as f64
    );
    println!(
        "drained over {:.1} s of wall clock",
        started.elapsed().as_secs_f64()
    );
}
