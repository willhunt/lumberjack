//! Turn recorded runs into CSV files somebody can plot.
//!
//! A results database holds every run of a project, which is what makes it good
//! to record into and awkward to hand to a spreadsheet. This writes one CSV per
//! run, named for when the run started, and leaves alone any run that has been
//! written out already: exporting twice costs nothing and does nothing.
//!
//! The database is opened read only. An export must never be able to damage the
//! results it is reading.
//!
//! ## What a file looks like
//!
//! ```text
//! timestamp_utc,seconds,Rig/Flow (L/min),Rig/Pressure (bar)
//! 2026-08-26 20:54:12.000,0.000000,14.5,7.25
//! 2026-08-26 20:54:12.050,0.050000,14.5,7.31
//! ```
//!
//! Values are written as they are held, to whatever precision that takes. A
//! reading is what the instrument said and rounding it on the way out would be
//! this deciding how much of it matters.
//!
//! `seconds` counts from the first reading of the run, which is what a test is
//! usually plotted against. The timestamp is there for anything that needs to
//! know when, and is UTC, as stored.
//!
//! One row per instant a reading was taken. Channels on the same device share
//! their timestamps exactly, so their columns line up with no gaps. Channels on
//! *different* devices never do — they are read by different threads and land
//! about half a millisecond apart — so a row belonging to one device leaves the
//! other's columns empty. That is the data telling the truth about itself
//! rather than a fault in the file.

use crate::Result;
use chrono::{ DateTime, TimeZone, Utc };
use rusqlite::{ Connection, OpenFlags };
use std::collections::BTreeSet;
use std::path::{ Path, PathBuf };

/// One recording in a results database.
pub struct Run {
    pub id: i64,
    pub started: DateTime<Utc>,
}

impl Run {
    /// What this run is called once it is a file.
    ///
    /// UTC, and said so, because the alternative bites twice a year: a run
    /// named in local time is exported as one name in winter and a different
    /// one in summer, so the same run comes out twice.
    pub fn file_name(&self) -> String {
        format!("{}.csv", self.started.format("%Y-%m-%d_%H-%M-%SZ"))
    }

    /// The same, with the run said outright.
    ///
    /// Only for two runs that started in the same second, which stopping and
    /// starting a recording quickly will do. Without it the second of them
    /// wants the first one file, and would be taken for it and never written.
    pub fn file_name_with_id(&self) -> String {
        format!("{}_run{}.csv", self.started.format("%Y-%m-%d_%H-%M-%SZ"), self.id)
    }
}

/// What one call to [`export`] did.
#[derive(Default)]
pub struct Exported {
    /// Files written, with how many rows went into each.
    pub written: Vec<(PathBuf, usize)>,
    /// Runs left alone because their file was already there.
    pub skipped: Vec<PathBuf>,
}

/// Write every run that has not been written out already.
///
/// `into` is created if it is not there. A run whose file already exists is
/// skipped: the file is taken as proof it was exported, so removing one is how
/// to have it written again.
pub fn export(database: &Path, into: &Path) -> Result<Exported> {
    // Read only. Whatever goes wrong here, it must not be the results.
    let connection = Connection::open_with_flags(
        database,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )?;

    let mut report = Exported::default();
    // A database opened but never recorded into has no tables at all, which is
    // nothing to export rather than something to fail over.
    if !recorded(&connection)? {
        return Ok(report);
    }
    let runs = runs(&connection)?;
    if runs.is_empty() {
        return Ok(report);
    }
    std::fs::create_dir_all(into)?;

    let mut taken: BTreeSet<String> = BTreeSet::new();
    for run in runs.iter() {
        let mut name = run.file_name();
        if !taken.insert(name.clone()) {
            name = run.file_name_with_id();
            taken.insert(name.clone());
        }
        let path = into.join(name);
        if path.exists() {
            report.skipped.push(path);
            continue;
        }
        let rows = write_run(&connection, run, &path)?;
        report.written.push((path, rows));
    }
    Ok(report)
}

/// Whether anything has ever been recorded into this database.
///
/// The tables are made when a run starts, so a file that exists but holds none
/// of them has simply never been recorded into.
fn recorded(connection: &Connection) -> Result<bool> {
    let found: i64 = connection.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'runs'",
        [],
        |row| row.get(0),
    )?;
    Ok(found > 0)
}

/// Every run in the database, oldest first.
pub fn runs(connection: &Connection) -> Result<Vec<Run>> {
    let mut statement = connection.prepare("SELECT id, started FROM runs ORDER BY id")?;
    let rows = statement.query_map([], |row| {
        let id: i64 = row.get(0)?;
        let started: String = row.get(1)?;
        Ok((id, started))
    })?;

    let mut runs = Vec::new();
    for row in rows {
        let (id, started) = row?;
        // Written by this crate as rfc3339, so anything else means the file was
        // meddled with and guessing would be worse than saying so.
        let started = DateTime::parse_from_rfc3339(&started)
            .map_err(|error| format!("run {} has an unreadable start time: {}", id, error))?
            .with_timezone(&Utc);
        runs.push(Run { id: id, started: started });
    }
    Ok(runs)
}

/// A column of the exported file: which channel it is and what it is called.
struct Column {
    channel_id: i64,
    heading: String,
}

fn columns(connection: &Connection, run: &Run) -> Result<Vec<Column>> {
    let mut statement = connection.prepare(
        "SELECT c.id, d.name, c.name, c.unit
           FROM channels c
           JOIN devices d ON d.id = c.device_id
          WHERE d.run_id = ?1
          ORDER BY d.id, c.id",
    )?;
    let rows = statement.query_map([run.id], |row| {
        let channel_id: i64 = row.get(0)?;
        let device: String = row.get(1)?;
        let channel: String = row.get(2)?;
        let unit: String = row.get(3)?;
        // The same way a channel is named everywhere else, plus the unit, so a
        // plot drawn from this has its axis labelled without being told.
        Ok(Column {
            channel_id: channel_id,
            heading: format!("{}/{} ({})", device, channel, unit),
        })
    })?;
    rows.collect::<std::result::Result<Vec<Column>, _>>().map_err(Into::into)
}

/// Write one run, returning how many rows it came to.
///
/// Readings are taken in timestamp order and turned into rows as they arrive,
/// so a run of any length costs one row of memory rather than all of it.
fn write_run(connection: &Connection, run: &Run, path: &Path) -> Result<usize> {
    let columns = columns(connection, run)?;
    let mut writer = csv::Writer::from_path(path)?;

    let mut heading = vec!["timestamp_utc".to_string(), "seconds".to_string()];
    heading.extend(columns.iter().map(|column| column.heading.clone()));
    writer.write_record(&heading)?;

    let mut statement = connection.prepare(
        "SELECT r.timestamp, r.channel_id, r.value
           FROM readings r
           JOIN channels c ON c.id = r.channel_id
           JOIN devices d ON d.id = c.device_id
          WHERE d.run_id = ?1
          ORDER BY r.timestamp",
    )?;
    let mut rows = statement.query([run.id])?;

    let mut written = 0;
    let mut first: Option<i64> = None;
    let mut at: Option<i64> = None;
    let mut cells: Vec<String> = vec![String::new(); columns.len()];

    while let Some(row) = rows.next()? {
        let timestamp: i64 = row.get(0)?;
        let channel_id: i64 = row.get(1)?;
        let value: f64 = row.get(2)?;

        if at != Some(timestamp) {
            if let Some(previous) = at {
                write_row(&mut writer, previous, first.unwrap_or(previous), &mut cells)?;
                written += 1;
            }
            first.get_or_insert(timestamp);
            at = Some(timestamp);
        }
        if let Some(position) = columns.iter().position(|column| column.channel_id == channel_id) {
            // Debug rather than Display: both give the shortest text that
            // reads back as the same f64, but Display spells 1.2e-16 out in
            // full as sixteen leading zeros.
            cells[position] = format!("{:?}", value);
        }
    }
    if let Some(last) = at {
        write_row(&mut writer, last, first.unwrap_or(last), &mut cells)?;
        written += 1;
    }

    writer.flush()?;
    Ok(written)
}

/// Write one instant, and empty the cells ready for the next.
///
/// Cleared rather than carried forward: a blank says no reading was taken then,
/// where repeating the last one would invent data that looks perfectly ordinary
/// and is not there.
fn write_row(
    writer: &mut csv::Writer<std::fs::File>,
    timestamp: i64,
    first: i64,
    cells: &mut [String],
) -> Result<()> {
    let when = Utc
        .timestamp_micros(timestamp)
        .single()
        // A space and no zone marker, because a spreadsheet reads that as a
        // time and reads rfc3339 as a piece of text. The column heading is
        // where it says these are UTC.
        // Microseconds, because that is what a reading is stamped with. Cut to
        // milliseconds, two readings a few hundred microseconds apart come out
        // looking like the same instant twice.
        .map(|when| when.format("%Y-%m-%d %H:%M:%S%.6f").to_string())
        .unwrap_or_default();
    let seconds = (timestamp - first) as f64 / 1_000_000.0;

    let mut record = vec![when, format!("{:.6}", seconds)];
    record.extend(cells.iter().cloned());
    writer.write_record(&record)?;
    for cell in cells.iter_mut() {
        cell.clear();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DaqConfig;
    use crate::datapoint::DataPoint;
    use crate::storage::{ Batch, DataSink };
    use crate::storage_sqlite::SqliteSink;

    /// A directory of this test own, so tests writing files cannot collide.
    fn scratch(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!("lumberdaq_export_{}", name));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        directory
    }

    /// One mock device with two channels, and optionally a second device.
    fn config(second: bool) -> DaqConfig {
        let device = |name: &str, channels: &[&str]| {
            format!(
                r#"{{ "info": {{ "name": "{}", "description": "-" }},
                      "read_interval_ms": 100,
                      "hardware": {{ "type": "MockHardware", "description": "-",
                        "acquisition": {{ "mode": "polled" }},
                        "channels": [ {} ] }} }}"#,
                name,
                channels
                    .iter()
                    .map(|channel| format!(
                        r#"{{ "name": "{}", "unit": "bar", "description": "-",
                              "input": {{ "Constant": 1.0 }} }}"#,
                        channel
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        let mut devices = vec![device("Rig", &["Flow", "Pressure"])];
        if second {
            devices.push(device("Other", &["Temp"]));
        }
        serde_json::from_str(&format!(
            r#"{{ "info": {{ "name": "Test", "author": "-" }}, "devices": [{}] }}"#,
            devices.join(", ")
        ))
        .expect("test config should parse")
    }

    fn batch(device: &str, channel: &str, at: &[i64], value: f64) -> Batch {
        Batch {
            device: device.to_string(),
            channel: channel.to_string(),
            datapoints: at
                .iter()
                .map(|micros| DataPoint {
                    datetime: Utc.timestamp_micros(*micros).unwrap(),
                    value: value,
                })
                .collect(),
        }
    }

    /// Record a run into a database the way a real run would, so that what is
    /// exported went in through the sink rather than through hand written SQL.
    /// A test using its own INSERTs would keep passing after the schema moved
    /// underneath it, which is the whole risk of reading a database from
    /// somewhere else.
    fn record(database: &Path, second: bool, at: &[i64]) {
        let mut sink = SqliteSink::new(&database.to_path_buf()).unwrap();
        let config = config(second);
        sink.init(&config).unwrap();
        sink.write_batch(&batch("Rig", "Flow", at, 14.5)).unwrap();
        sink.write_batch(&batch("Rig", "Pressure", at, 7.25)).unwrap();
        if second {
            // Half a millisecond later, which is what two devices read by two
            // threads really look like.
            let staggered: Vec<i64> = at.iter().map(|micros| micros + 500).collect();
            sink.write_batch(&batch("Other", "Temp", &staggered, 20.0)).unwrap();
        }
        sink.flush().unwrap();
    }

    fn lines(path: &Path) -> Vec<String> {
        std::fs::read_to_string(path).unwrap().lines().map(|line| line.to_string()).collect()
    }

    #[test]
    fn a_run_comes_out_as_one_file_named_for_when_it_started() {
        let directory = scratch("one_run");
        let database = directory.join("results.db");
        record(&database, false, &[1_000_000, 1_100_000, 1_200_000]);

        let report = export(&database, &directory.join("export")).unwrap();
        assert_eq!(report.written.len(), 1);
        assert_eq!(report.skipped.len(), 0);

        let (path, rows) = &report.written[0];
        assert_eq!(*rows, 3);
        let name = path.file_name().unwrap().to_string_lossy();
        assert!(name.ends_with("Z.csv"), "should say it is UTC: {}", name);

        let lines = lines(path);
        assert_eq!(
            lines[0],
            "timestamp_utc,seconds,Rig/Flow (bar),Rig/Pressure (bar)"
        );
        // Channels on one device share a timestamp exactly, so no gaps.
        assert_eq!(lines[1], "1970-01-01 00:00:01.000000,0.000000,14.5,7.25");
        assert_eq!(lines[2], "1970-01-01 00:00:01.100000,0.100000,14.5,7.25");
        assert_eq!(lines.len(), 4);
    }

    #[test]
    fn seconds_count_from_the_first_reading_of_that_run() {
        // What a test is plotted against, and it has to start at zero for each
        // run rather than carry on from the one before.
        let directory = scratch("seconds");
        let database = directory.join("results.db");
        record(&database, false, &[9_000_000, 9_250_000]);
        let report = export(&database, &directory.join("export")).unwrap();
        let lines = lines(&report.written[0].0);
        assert!(lines[1].contains(",0.000000,"), "{}", lines[1]);
        assert!(lines[2].contains(",0.250000,"), "{}", lines[2]);
    }

    #[test]
    fn a_run_already_written_out_is_left_alone() {
        let directory = scratch("skip");
        let database = directory.join("results.db");
        let into = directory.join("export");
        record(&database, false, &[1_000_000]);

        let first = export(&database, &into).unwrap();
        assert_eq!(first.written.len(), 1);
        let second = export(&database, &into).unwrap();
        assert_eq!(second.written.len(), 0);
        assert_eq!(second.skipped.len(), 1);
    }

    #[test]
    fn removing_a_file_is_how_to_have_it_written_again() {
        // The file is the record of what has been exported, so there is nothing
        // else to reset.
        let directory = scratch("again");
        let database = directory.join("results.db");
        let into = directory.join("export");
        record(&database, false, &[1_000_000]);

        let path = export(&database, &into).unwrap().written[0].0.clone();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(export(&database, &into).unwrap().written.len(), 1);
    }

    #[test]
    fn each_run_gets_its_own_file() {
        let directory = scratch("two_runs");
        let database = directory.join("results.db");
        record(&database, false, &[1_000_000]);
        // A second later, or the two runs would want the same name.
        record(&database, false, &[2_000_000]);

        let report = export(&database, &directory.join("export")).unwrap();
        assert_eq!(report.written.len(), 2);
        assert_ne!(report.written[0].0, report.written[1].0);
    }

    #[test]
    fn channels_on_different_devices_leave_gaps_rather_than_being_lined_up() {
        // They are read by different threads and land a fraction of a
        // millisecond apart. Putting them on one row would be inventing an
        // alignment the data does not have.
        let directory = scratch("gaps");
        let database = directory.join("results.db");
        record(&database, true, &[1_000_000]);

        let report = export(&database, &directory.join("export")).unwrap();
        let lines = lines(&report.written[0].0);
        assert_eq!(lines[0], "timestamp_utc,seconds,Rig/Flow (bar),Rig/Pressure (bar),Other/Temp (bar)");
        // One row for the pair that share an instant, one for the one that
        // does not, each blank where the other device has nothing to say.
        assert_eq!(lines[1], "1970-01-01 00:00:01.000000,0.000000,14.5,7.25,");
        assert_eq!(lines[2], "1970-01-01 00:00:01.000500,0.000500,,,20.0");
    }

    #[test]
    fn a_blank_is_not_the_last_reading_repeated() {
        // Carrying a value forward would invent data that looks exactly like a
        // real reading.
        let directory = scratch("no_carry");
        let database = directory.join("results.db");
        record(&database, true, &[1_000_000, 1_100_000]);
        let report = export(&database, &directory.join("export")).unwrap();
        let text = std::fs::read_to_string(&report.written[0].0).unwrap();
        assert_eq!(text.matches("20.0").count(), 2, "one per reading, not one per row");
    }

    #[test]
    fn a_database_with_nothing_recorded_yet_exports_nothing() {
        let directory = scratch("empty");
        let database = directory.join("results.db");
        // Opening a sink creates the tables but records no run.
        let _ = SqliteSink::new(&database).unwrap();

        let into = directory.join("export");
        let report = export(&database, &into).unwrap();
        assert!(report.written.is_empty());
        assert!(report.skipped.is_empty());
        assert!(!into.exists(), "nothing to put there, so no directory either");
    }

    #[test]
    fn exporting_cannot_write_to_the_results() {
        // Opened read only on purpose. Whatever goes wrong in here, it must not
        // be the recording.
        let directory = scratch("read_only");
        let database = directory.join("results.db");
        record(&database, false, &[1_000_000]);
        export(&database, &directory.join("export")).unwrap();

        let connection = Connection::open_with_flags(
            &database,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
        )
        .unwrap();
        assert!(connection.execute("DELETE FROM readings", []).is_err());
    }

    #[test]
    fn a_run_is_named_in_utc_whatever_the_machine_thinks_the_time_is() {
        // Named in local time, the same run comes out under one name in winter
        // and another in summer, and so gets exported twice.
        let run = Run { id: 1, started: Utc.timestamp_micros(1_800_000_000_000_000).unwrap() };
        assert_eq!(run.file_name(), "2027-01-15_08-00-00Z.csv");
    }
}
