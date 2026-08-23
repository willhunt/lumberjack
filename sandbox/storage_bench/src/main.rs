//! CSV against SQLite for lumberdaq shaped data.
//!
//! storage_csv.rs says csv "is much faster than saving a SQLite data base to
//! disk". That is true of the obvious way to use SQLite and false of the usual
//! way, so this measures both rather than leaving it as folklore.
//!
//! The workload copies the real acquisition loop: every cycle produces one
//! datapoint per channel, all sharing a timestamp, and every row repeats the
//! device and channel name exactly as the long format csv does.
//!
//! What separates the strategies is mostly *durability*, so they are grouped by
//! how much a crash would cost rather than listed flat. Comparing a csv that
//! flushes every cycle against a SQLite file that commits once at the end is
//! not a fair fight, and is roughly the comparison that produces the folklore.
//!
//! Run it in release mode. Debug numbers are meaningless here:
//!     cargo run --release -- [cycles]

use chrono::{ DateTime, TimeZone, Utc };
use rusqlite::Connection;
use std::path::{ Path, PathBuf };
use std::time::{ Duration, Instant };

/// Channels per device, matching a small serial rig.
const CHANNELS: usize = 8;
/// Sample period, only used to make the timestamps look realistic.
const PERIOD_MICROS: i64 = 1_000;
/// Naive SQLite fsyncs per row, so it is thousands of times slower than
/// everything else. Give it a smaller share of the work and scale the result.
const NAIVE_CYCLE_CAP: usize = 400;

fn main() {
    let cycles: usize = std::env::args()
        .nth(1)
        .and_then(|arg| arg.parse().ok())
        .unwrap_or(20_000);
    let rows = cycles * CHANNELS;

    let dir = PathBuf::from("bench_output");
    std::fs::create_dir_all(&dir).expect("could not create bench_output");

    println!("lumberdaq storage benchmark");
    println!("{} cycles x {} channels = {} rows\n", cycles, CHANNELS, rows);

    let mut results: Vec<Outcome> = Vec::new();

    // ---- Durable every cycle -------------------------------------------------
    // What the acquisition loop actually wants: a crash costs at most one cycle.
    // Note the two csv rows are not equally safe. Only the synced one matches
    // what SQLite does on commit.
    results.push(run("csv, flush every cycle (no fsync)", Durability::Cycle, || {
        csv_run(&dir.join("cycle.csv"), cycles, FlushPolicy::EveryCycle)
    }));
    results.push(run("csv, flush + fsync every cycle", Durability::Cycle, || {
        csv_run(&dir.join("cycle_sync.csv"), cycles, FlushPolicy::EveryCycleSynced)
    }));
    results.push(run("sqlite, commit every cycle", Durability::Cycle, || {
        sqlite_run(&dir.join("cycle.db"), cycles, Commit::EveryCycle, TimeFormat::Text, Journal::Delete)
    }));
    results.push(run("sqlite, commit every cycle, WAL", Durability::Cycle, || {
        sqlite_run(&dir.join("cycle_wal.db"), cycles, Commit::EveryCycle, TimeFormat::Text, Journal::Wal)
    }));
    results.push(run("sqlite, commit every cycle, WAL, integer time", Durability::Cycle, || {
        sqlite_run(&dir.join("cycle_wal_int.db"), cycles, Commit::EveryCycle, TimeFormat::Micros, Journal::Wal)
    }));

    // ---- Durable at the end only --------------------------------------------
    // The throughput ceiling. A crash loses the whole run, so this is the wrong
    // setting for acquisition, but it is what a one-shot export would use.
    results.push(run("csv, flush at end", Durability::End, || {
        csv_run(&dir.join("end.csv"), cycles, FlushPolicy::AtEnd)
    }));
    results.push(run("sqlite, one transaction", Durability::End, || {
        sqlite_run(&dir.join("end.db"), cycles, Commit::AtEnd, TimeFormat::Text, Journal::Delete)
    }));
    results.push(run("sqlite, one transaction, integer time", Durability::End, || {
        sqlite_run(&dir.join("end_int.db"), cycles, Commit::AtEnd, TimeFormat::Micros, Journal::Delete)
    }));

    // ---- Durable every row ---------------------------------------------------
    // csv here is the old write_csv_record, which flushed per datapoint.
    // SQLite here is the naive version: no transaction, so every insert is its
    // own commit and its own fsync. This is almost certainly what the earlier
    // testing measured.
    results.push(run("csv, flush every row", Durability::Row, || {
        csv_run(&dir.join("row.csv"), cycles, FlushPolicy::EveryRow)
    }));

    let naive_cycles = cycles.min(NAIVE_CYCLE_CAP);
    let naive = run("sqlite, no transaction", Durability::Row, || {
        sqlite_run(&dir.join("row.db"), naive_cycles, Commit::EveryRow, TimeFormat::Text, Journal::Delete)
    });
    let naive = if naive_cycles < cycles {
        println!(
            "  (ran {} of {} cycles and scaled: it fsyncs per row)",
            naive_cycles, cycles
        );
        naive.scaled_to(cycles as f64 / naive_cycles as f64)
    } else {
        naive
    };
    results.push(naive);

    report(&results, rows);
    read_back(&dir, cycles);
}

// -----------------------------------------------------------------------------
// Workload
// -----------------------------------------------------------------------------

/// Deterministic values, so every strategy writes identical data.
struct Values(u64);

impl Values {
    fn new() -> Values {
        Values(0x2545F4914F6CDD1D)
    }
    fn next(&mut self) -> f64 {
        // xorshift; cheap enough not to show up in the measurement.
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn channel_names() -> Vec<String> {
    (0..CHANNELS).map(|index| format!("Channel {}", index)).collect()
}

fn base_time() -> DateTime<Utc> {
    Utc.timestamp_opt(1_700_000_000, 0).unwrap()
}

fn timestamp_for(cycle: usize) -> DateTime<Utc> {
    base_time() + Duration::from_micros((cycle as i64 * PERIOD_MICROS) as u64)
}

// -----------------------------------------------------------------------------
// CSV
// -----------------------------------------------------------------------------

enum FlushPolicy {
    EveryRow,
    EveryCycle,
    /// Flush *and* fsync every cycle.
    ///
    /// This is the only csv setting comparable to a SQLite commit. A plain
    /// `flush` only hands the bytes to the operating system, which survives the
    /// process dying but not the machine losing power; SQLite forces them all
    /// the way down on every commit. Comparing the two without this row is the
    /// mistake that makes csv look untouchable.
    EveryCycleSynced,
    AtEnd,
}

fn csv_run(path: &Path, cycles: usize, policy: FlushPolicy) -> u64 {
    let _ = std::fs::remove_file(path);
    let mut writer = csv::Writer::from_path(path).expect("could not open csv");
    writer
        .write_record(&["Device", "Channel", "Timestamp", "Value"])
        .expect("could not write header");

    // A second handle to the same file, purely to force the fsync. fsync acts
    // on the file, not on the handle that wrote to it.
    let syncer = match policy {
        FlushPolicy::EveryCycleSynced => Some(
            std::fs::OpenOptions::new()
                .write(true)
                .open(path)
                .expect("could not open csv for syncing"),
        ),
        _ => None,
    };

    let channels = channel_names();
    let mut values = Values::new();

    for cycle in 0..cycles {
        let timestamp = timestamp_for(cycle).to_string();
        for channel in channels.iter() {
            writer
                .write_record(&[
                    "Serial test device",
                    channel,
                    &timestamp,
                    &values.next().to_string(),
                ])
                .expect("could not write row");
            if matches!(policy, FlushPolicy::EveryRow) {
                writer.flush().expect("could not flush");
            }
        }
        match policy {
            FlushPolicy::EveryCycle => writer.flush().expect("could not flush"),
            FlushPolicy::EveryCycleSynced => {
                writer.flush().expect("could not flush");
                syncer.as_ref().unwrap().sync_all().expect("could not fsync");
            }
            _ => {}
        }
    }
    writer.flush().expect("could not flush");
    drop(writer);
    file_size(path)
}

// -----------------------------------------------------------------------------
// SQLite
// -----------------------------------------------------------------------------

enum Commit {
    EveryRow,
    EveryCycle,
    AtEnd,
}

/// Storing the timestamp as text keeps parity with the csv; storing it as
/// microseconds is smaller and avoids formatting a string per row.
enum TimeFormat {
    Text,
    Micros,
}

enum Journal {
    Delete,
    Wal,
}

fn sqlite_run(
    path: &Path,
    cycles: usize,
    commit: Commit,
    time_format: TimeFormat,
    journal: Journal,
) -> u64 {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(path.with_extension("db-wal"));
    let _ = std::fs::remove_file(path.with_extension("db-shm"));

    let mut connection = Connection::open(path).expect("could not open database");
    if matches!(journal, Journal::Wal) {
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .expect("could not set WAL");
    }
    // No index: the csv has none either, so adding one here would not be a fair
    // comparison of write cost.
    let column = match time_format {
        TimeFormat::Text => "timestamp TEXT NOT NULL",
        TimeFormat::Micros => "timestamp INTEGER NOT NULL",
    };
    connection
        .execute_batch(&format!(
            "CREATE TABLE readings (
                 device  TEXT NOT NULL,
                 channel TEXT NOT NULL,
                 {},
                 value   REAL NOT NULL
             );",
            column
        ))
        .expect("could not create table");

    let channels = channel_names();
    let mut values = Values::new();

    match commit {
        // Every insert is its own implicit transaction, so every insert fsyncs.
        Commit::EveryRow => {
            let statement = "INSERT INTO readings VALUES (?1, ?2, ?3, ?4)";
            for cycle in 0..cycles {
                let time = timestamp_for(cycle);
                for channel in channels.iter() {
                    insert(&connection, statement, channel, time, &time_format, values.next());
                }
            }
        }
        Commit::EveryCycle => {
            let statement = "INSERT INTO readings VALUES (?1, ?2, ?3, ?4)";
            for cycle in 0..cycles {
                let time = timestamp_for(cycle);
                let transaction = connection.transaction().expect("could not begin");
                for channel in channels.iter() {
                    insert(&transaction, statement, channel, time, &time_format, values.next());
                }
                transaction.commit().expect("could not commit");
            }
        }
        Commit::AtEnd => {
            let statement = "INSERT INTO readings VALUES (?1, ?2, ?3, ?4)";
            let transaction = connection.transaction().expect("could not begin");
            for cycle in 0..cycles {
                let time = timestamp_for(cycle);
                for channel in channels.iter() {
                    insert(&transaction, statement, channel, time, &time_format, values.next());
                }
            }
            transaction.commit().expect("could not commit");
        }
    }
    drop(connection);

    // WAL keeps recent writes in a sidecar file, so count it towards the size.
    file_size(path)
        + file_size(&path.with_extension("db-wal"))
        + file_size(&path.with_extension("db-shm"))
}

/// `prepare_cached` matters: preparing the statement per row would measure the
/// SQL parser rather than the storage engine.
fn insert(
    connection: &Connection,
    statement: &str,
    channel: &str,
    time: DateTime<Utc>,
    time_format: &TimeFormat,
    value: f64,
) {
    let mut prepared = connection
        .prepare_cached(statement)
        .expect("could not prepare");
    match time_format {
        TimeFormat::Text => prepared
            .execute(rusqlite::params!["Serial test device", channel, time.to_string(), value]),
        TimeFormat::Micros => prepared
            .execute(rusqlite::params!["Serial test device", channel, time.timestamp_micros(), value]),
    }
    .expect("could not insert");
}

// -----------------------------------------------------------------------------
// Reading back
// -----------------------------------------------------------------------------

/// Write speed is only half the question. This is the other half: getting one
/// channel back out, which is what any analysis or plot has to do.
fn read_back(dir: &Path, cycles: usize) {
    println!("\nReading one channel back out of {} rows", cycles * CHANNELS);

    let csv_path = dir.join("end.csv");
    let start = Instant::now();
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_path(&csv_path)
        .expect("could not open csv");
    let mut count = 0usize;
    let mut total = 0f64;
    for record in reader.records() {
        let record = record.expect("bad record");
        if &record[1] == "Channel 3" {
            total += record[3].parse::<f64>().expect("bad value");
            count += 1;
        }
    }
    println!(
        "  csv     {:>9.1} ms   {} rows (whole file scanned and parsed)",
        start.elapsed().as_secs_f64() * 1000.0,
        count
    );
    let _ = total;

    let db_path = dir.join("end.db");
    let connection = Connection::open(&db_path).expect("could not open database");
    let start = Instant::now();
    let (count, total): (i64, f64) = connection
        .query_row(
            "SELECT COUNT(*), COALESCE(SUM(value), 0) FROM readings WHERE channel = ?1",
            ["Channel 3"],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("query failed");
    println!(
        "  sqlite  {:>9.1} ms   {} rows (no index, so also a full scan)",
        start.elapsed().as_secs_f64() * 1000.0,
        count
    );
    let _ = total;

    let start = Instant::now();
    connection
        .execute_batch("CREATE INDEX idx_channel ON readings(channel);")
        .expect("could not index");
    let index_time = start.elapsed();
    let start = Instant::now();
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM readings WHERE channel = ?1",
            ["Channel 3"],
            |row| row.get(0),
        )
        .expect("query failed");
    println!(
        "  sqlite  {:>9.1} ms   {} rows (indexed; index built in {:.1} ms)",
        start.elapsed().as_secs_f64() * 1000.0,
        count,
        index_time.as_secs_f64() * 1000.0
    );
}

// -----------------------------------------------------------------------------
// Plumbing
// -----------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Durability {
    Row,
    Cycle,
    End,
}

impl Durability {
    fn label(&self) -> &'static str {
        match self {
            Durability::Row => "a crash loses nothing",
            Durability::Cycle => "a crash loses one cycle",
            Durability::End => "a crash loses everything",
        }
    }
}

struct Outcome {
    name: &'static str,
    durability: Durability,
    elapsed: f64,
    bytes: u64,
}

impl Outcome {
    fn scaled_to(self, factor: f64) -> Outcome {
        Outcome {
            elapsed: self.elapsed * factor,
            bytes: (self.bytes as f64 * factor) as u64,
            ..self
        }
    }
}

fn run<F: FnOnce() -> u64>(name: &'static str, durability: Durability, task: F) -> Outcome {
    print!("  running {:<45}", name);
    use std::io::Write;
    std::io::stdout().flush().ok();
    let start = Instant::now();
    let bytes = task();
    let elapsed = start.elapsed().as_secs_f64();
    println!("{:>8.2} s", elapsed);
    Outcome { name: name, durability: durability, elapsed: elapsed, bytes: bytes }
}

fn report(results: &[Outcome], rows: usize) {
    println!("\n{:-<86}", "");
    println!(
        "{:<45} {:>10} {:>12} {:>10}",
        "strategy", "seconds", "rows/sec", "file"
    );
    println!("{:-<86}", "");

    for durability in [Durability::Cycle, Durability::End, Durability::Row] {
        println!("\n  {}", durability.label());
        for outcome in results.iter().filter(|r| r.durability == durability) {
            println!(
                "{:<45} {:>10.2} {:>12} {:>10}",
                outcome.name,
                outcome.elapsed,
                thousands((rows as f64 / outcome.elapsed) as u64),
                human_bytes(outcome.bytes),
            );
        }
    }
    println!("\n{:-<86}", "");
}

fn file_size(path: &Path) -> u64 {
    std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0)
}

fn human_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.0} KB", bytes as f64 / 1024.0)
    }
}

fn thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}
