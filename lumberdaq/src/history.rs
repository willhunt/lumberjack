//! Reading a results database back.
//!
//! The other half of [`storage_sqlite`](crate::storage_sqlite), which writes
//! one. That module knows how a run is recorded; this one knows how to look at
//! what was recorded, in the shape somebody inspecting it thinks in: a run
//! holds devices, a device holds channels, a channel holds readings.
//!
//! Distinct from [`export`](crate::export), which reads the same file for a
//! different purpose — to turn a whole run into a csv, in one pass, without
//! anybody choosing anything. This is for picking through it.
//!
//! Opened read only, always. Nothing here should be able to damage a result,
//! and a viewer looking at a database while a run is recording into it must
//! not be able to get in the way.

use crate::datapoint::DataPoint;
use crate::Result;
use chrono::{ TimeZone, Utc };
use rusqlite::{ Connection, OpenFlags };
use std::path::Path;

pub use crate::export::Run;

/// One device as it was recorded.
///
/// `hardware` is the backend configuration as it stood, kept as the json the
/// run was recorded with. Names alone cannot say whether two runs measured the
/// same thing; this can, which is what makes a results file able to describe
/// its own setup.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordedDevice {
    pub id: i64,
    pub name: String,
    pub hardware: String,
}

/// One channel as it was recorded, and what its numbers are in.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordedChannel {
    pub id: i64,
    pub name: String,
    pub unit: String,
}

/// A results database, open for looking at.
#[derive(Debug)]
pub struct Archive {
    connection: Connection,
}

impl Archive {
    /// Open a results database to read.
    ///
    /// Read only and explicitly so, rather than by good behaviour: a viewer
    /// has no business writing here, and sqlite will refuse rather than trust
    /// us. It also means a run recording into this same file carries on
    /// undisturbed, which is what lets both modes be open at once.
    pub fn open(path: &Path) -> Result<Archive> {
        let connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
        )?;

        // Checked here rather than met as a missing column three queries
        // later, which would report whichever column happened to be absent
        // instead of the actual problem.
        //
        // A range, not a match. Reading an older recording is the whole point
        // of a viewer: those files are records of experiments that cannot be
        // taken again, and refusing them because a column somebody never reads
        // was dropped afterwards would be losing them for nothing. See
        // `READABLE_FROM` for what each version changed.
        let found: i32 =
            connection.query_row("PRAGMA user_version", [], |row| row.get(0)).unwrap_or(0);

        if found < crate::storage_sqlite::READABLE_FROM {
            return Err(crate::Error::DatabaseTooOld {
                found,
                oldest: crate::storage_sqlite::READABLE_FROM,
            });
        }
        // Newer than us is a different problem: we cannot know what changed,
        // so guessing is worse than saying so.
        if found > crate::storage_sqlite::SCHEMA_VERSION {
            return Err(crate::Error::DatabaseSchemaVersion {
                found,
                expected: crate::storage_sqlite::SCHEMA_VERSION,
            });
        }

        Ok(Archive { connection })
    }

    /// Every run in the file, oldest first.
    pub fn runs(&self) -> Result<Vec<Run>> {
        crate::export::runs(&self.connection)
    }

    /// The devices recorded in one run, in the order they were written.
    pub fn devices(&self, run: i64) -> Result<Vec<RecordedDevice>> {
        let mut statement = self
            .connection
            .prepare("SELECT id, name, hardware FROM devices WHERE run_id = ?1 ORDER BY id")?;

        let rows = statement.query_map([run], |row| {
            Ok(RecordedDevice { id: row.get(0)?, name: row.get(1)?, hardware: row.get(2)? })
        })?;

        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// The channels of one device, in the order they were written.
    pub fn channels(&self, device: i64) -> Result<Vec<RecordedChannel>> {
        let mut statement = self
            .connection
            .prepare("SELECT id, name, unit FROM channels WHERE device_id = ?1 ORDER BY id")?;

        let rows = statement.query_map([device], |row| {
            Ok(RecordedChannel { id: row.get(0)?, name: row.get(1)?, unit: row.get(2)? })
        })?;

        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// How many readings one channel has.
    ///
    /// Worth asking before asking for them: a channel read at 10 ms for an
    /// afternoon holds several million, and whoever is about to draw them may
    /// want to say so rather than do it.
    pub fn reading_count(&self, channel: i64) -> Result<usize> {
        let count: i64 = self.connection.query_row(
            "SELECT COUNT(*) FROM readings WHERE channel_id = ?1",
            [channel],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    /// Every reading of one channel, oldest first.
    ///
    /// Stamped in microseconds, which is what a `DataPoint` carries and what
    /// was written. A timestamp that will not convert means the file was
    /// meddled with, and saying so is better than guessing at a time.
    pub fn readings(&self, channel: i64) -> Result<Vec<DataPoint>> {
        let mut statement = self.connection.prepare(
            "SELECT timestamp, value FROM readings WHERE channel_id = ?1 ORDER BY timestamp",
        )?;

        let rows = statement
            .query_map([channel], |row| {
                let micros: i64 = row.get(0)?;
                let value: f64 = row.get(1)?;
                Ok((micros, value))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let mut readings = Vec::with_capacity(rows.len());
        for (micros, value) in rows {
            let datetime = Utc.timestamp_micros(micros).single().ok_or_else(|| {
                format!("channel {} has a reading with an unreadable timestamp", channel)
            })?;
            readings.push(DataPoint { datetime, value });
        }
        Ok(readings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DaqConfig;
    use crate::storage::{ Batch, DataSink };
    use crate::storage_sqlite::SqliteSink;

    /// A rig with two channels on one device.
    fn config() -> DaqConfig {
        serde_json::from_str(
            r#"{
              "info": { "name": "History", "author": "Test" },
              "devices": [{
                "info": { "name": "Rig" },
                "hardware": {
                  "type": "MockHardware",
                  "channels": [
                    { "name": "Flow", "unit": "L/min", "input": "Random" },
                    { "name": "Pressure", "unit": "bar", "input": "Random" }
                  ]
                }
              }]
            }"#,
        )
        .expect("test rig should parse")
    }

    /// Record a short run, and hand back the file it went into.
    fn recorded(into: &Path) {
        let mut sink = SqliteSink::new(into).expect("a results file should be creatable");
        sink.init(&config()).expect("the run should start");

        let started = Utc.timestamp_micros(1_800_000_000_000_000).unwrap();
        for (channel, values) in [("Flow", [1.0, 2.0, 3.0]), ("Pressure", [10.0, 20.0, 30.0])] {
            let datapoints = values
                .iter()
                .enumerate()
                .map(|(step, value)| DataPoint {
                    datetime: started + chrono::Duration::milliseconds(step as i64 * 100),
                    value: *value,
                })
                .collect();

            sink.write_batch(&Batch {
                device: "Rig".to_string(),
                channel: channel.to_string(),
                datapoints,
            })
            .expect("the batch should be written");
        }
        sink.flush().expect("the run should be flushed");
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("lumberdaq_history_{}.db", name));
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn a_recorded_run_reads_back_as_it_was_written() {
        let path = scratch("round_trip");
        recorded(&path);

        let archive = Archive::open(&path).expect("the file should open");
        let runs = archive.runs().expect("runs should read");
        assert_eq!(runs.len(), 1);

        let devices = archive.devices(runs[0].id).expect("devices should read");
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].name, "Rig");
        assert!(devices[0].hardware.contains("MockHardware"), "{}", devices[0].hardware);

        let channels = archive.channels(devices[0].id).expect("channels should read");
        assert_eq!(
            channels.iter().map(|channel| channel.name.as_str()).collect::<Vec<_>>(),
            vec!["Flow", "Pressure"]
        );
        assert_eq!(channels[0].unit, "L/min");

        let readings = archive.readings(channels[0].id).expect("readings should read");
        assert_eq!(readings.iter().map(|point| point.value).collect::<Vec<_>>(), vec![1.0, 2.0, 3.0]);
        assert_eq!(archive.reading_count(channels[0].id).unwrap(), 3);

        // Microseconds survive: the stamp is what was written, not a rounding
        // of it to the nearest second.
        assert_eq!(readings[1].datetime, readings[0].datetime + chrono::Duration::milliseconds(100));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn two_runs_in_one_file_keep_their_own_devices() {
        let path = scratch("two_runs");
        recorded(&path);
        recorded(&path);

        let archive = Archive::open(&path).expect("the file should open");
        let runs = archive.runs().expect("runs should read");
        assert_eq!(runs.len(), 2);

        let first = archive.devices(runs[0].id).expect("devices should read");
        let second = archive.devices(runs[1].id).expect("devices should read");

        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
        // Same name, different rows: a device belongs to its run, so recording
        // the same rig again is a second run rather than more of the first.
        assert_eq!(first[0].name, second[0].name);
        assert_ne!(first[0].id, second[0].id);

        let _ = std::fs::remove_file(&path);
    }

    /// A results file as version 2 wrote one: the oldest shape this build
    /// claims to read, with the columns that have since gone away still in it.
    fn old_schema(path: &Path, version: i32) {
        let old = rusqlite::Connection::open(path).expect("a file should be creatable");
        old.execute_batch(
            "CREATE TABLE runs (
                 id INTEGER PRIMARY KEY, name TEXT NOT NULL,
                 author TEXT NOT NULL, started TEXT NOT NULL);
             CREATE TABLE devices (
                 id INTEGER PRIMARY KEY, run_id INTEGER NOT NULL,
                 name TEXT NOT NULL, description TEXT NOT NULL,
                 hardware TEXT NOT NULL);
             CREATE TABLE channels (
                 id INTEGER PRIMARY KEY, device_id INTEGER NOT NULL,
                 hardware_id TEXT NOT NULL, name TEXT NOT NULL,
                 unit TEXT NOT NULL, description TEXT NOT NULL);
             CREATE TABLE readings (
                 channel_id INTEGER NOT NULL, timestamp INTEGER NOT NULL,
                 value REAL NOT NULL);

             INSERT INTO runs VALUES (1, 'Old', 'Test', '2020-01-01T00:00:00+00:00');
             INSERT INTO devices VALUES (1, 1, 'Rig', 'a description', '{\"type\":\"MockHardware\"}');
             INSERT INTO channels VALUES (1, 1, 'ai0', 'Flow', 'L/min', 'a description');
             INSERT INTO readings VALUES (1, 1577836800000000, 4.5);",
        )
        .expect("the old schema should build");

        old.pragma_update(None, "user_version", version).expect("a version should be settable");
    }

    #[test]
    fn a_recording_from_an_older_version_still_reads() {
        // The point of the whole exercise: those files are records of
        // experiments that cannot be taken again, and the columns dropped
        // since are ones nothing here ever selected.
        let path = scratch("version_2");
        old_schema(&path, 2);

        let archive = Archive::open(&path).expect("version 2 should still be readable");
        let runs = archive.runs().expect("runs should read");
        assert_eq!(runs.len(), 1);

        let devices = archive.devices(runs[0].id).expect("devices should read");
        assert_eq!(devices[0].name, "Rig");

        let channels = archive.channels(devices[0].id).expect("channels should read");
        assert_eq!(channels[0].unit, "L/min");

        let readings = archive.readings(channels[0].id).expect("readings should read");
        assert_eq!(readings.len(), 1);
        assert_eq!(readings[0].value, 4.5);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_recording_older_than_we_can_read_is_refused_by_name() {
        let path = scratch("version_1");
        old_schema(&path, 1);

        let refused = Archive::open(&path).expect_err("version 1 should be refused");
        assert!(
            matches!(refused, crate::Error::DatabaseTooOld { found: 1, oldest: 2 }),
            "{}",
            refused
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_recording_from_a_newer_version_is_refused_too() {
        // Not the same problem: what changed is unknown, so reading it anyway
        // would be guessing.
        let path = scratch("version_99");
        old_schema(&path, 99);

        let refused = Archive::open(&path).expect_err("a newer file should be refused");
        assert!(
            matches!(refused, crate::Error::DatabaseSchemaVersion { found: 99, .. }),
            "{}",
            refused
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn an_archive_cannot_be_written_to() {
        let path = scratch("read_only");
        recorded(&path);

        let archive = Archive::open(&path).expect("the file should open");
        let refused = archive.connection.execute("DELETE FROM readings", []);

        assert!(refused.is_err(), "a viewer must not be able to change a result");

        let _ = std::fs::remove_file(&path);
    }
}
