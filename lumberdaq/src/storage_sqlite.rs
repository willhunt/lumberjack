use crate::storage::{ Batch, DaqHeader, DataSink };
use crate::{ Error, Result };
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::Path;

/// Records to a SQLite database instead of a csv file plus a json sidecar.
///
/// The schema is normalised, which is the main difference from the csv. The
/// long format csv repeats the device name, the channel name and a formatted
/// timestamp on every row; here a reading is three numbers referring to a
/// channel that is described once.
///
/// ```text
/// daq       one row: the test name and author
/// devices   one row per device
/// channels  one row per channel, pointing at its device
/// readings  channel_id, timestamp, value
/// ```
///
/// Because the description lives in the same file as the data, there is no
/// sidecar to keep in step with it.
///
/// Timestamps are stored as microseconds since the epoch rather than as text:
/// smaller, and no string to format per row.
pub struct SqliteSink {
    connection: Connection,
    /// device name -> channel name -> row id, built once from the header so a
    /// batch does not have to look its channel up by string on every write.
    channel_ids: HashMap<String, HashMap<String, i64>>,
    /// Whether there is an open transaction waiting to be committed.
    uncommitted: bool,
}

impl SqliteSink {
    pub fn new(path: &Path) -> Result<SqliteSink> {
        let connection = Connection::open(path)?;

        // Write ahead logging, for two reasons. Readers do not block the
        // writer, so a UI can plot a run while it is still recording; and
        // commits append to a log rather than rewriting pages.
        connection.pragma_update(None, "journal_mode", "WAL")?;

        // NORMAL means a commit does not wait for the disk. A crashed process
        // still loses nothing, since the log is already handed to the OS; a
        // power cut loses the last few seconds. FULL would make commits durable
        // against power loss too, at roughly a millisecond each.
        connection.pragma_update(None, "synchronous", "NORMAL")?;

        Ok(SqliteSink {
            connection: connection,
            channel_ids: HashMap::new(),
            uncommitted: false,
        })
    }

    /// Open a transaction if one is not already open.
    ///
    /// Inserts outside a transaction get one each, and every transaction is a
    /// separate commit, which is thousands of times slower. Batching until
    /// `flush` is what keeps this fast.
    fn begin(&mut self) -> Result<()> {
        if !self.uncommitted {
            self.connection.execute_batch("BEGIN")?;
            self.uncommitted = true;
        }
        Ok(())
    }
}

impl DataSink for SqliteSink {
    fn init(&mut self, header: &DaqHeader) -> Result<()> {
        self.connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS daq (
                 name        TEXT NOT NULL,
                 author      TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS devices (
                 id          INTEGER PRIMARY KEY,
                 name        TEXT NOT NULL UNIQUE,
                 description TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS channels (
                 id          INTEGER PRIMARY KEY,
                 device_id   INTEGER NOT NULL REFERENCES devices(id),
                 name        TEXT NOT NULL,
                 unit        TEXT NOT NULL,
                 description TEXT NOT NULL,
                 UNIQUE(device_id, name)
             );
             CREATE TABLE IF NOT EXISTS readings (
                 channel_id  INTEGER NOT NULL REFERENCES channels(id),
                 timestamp   INTEGER NOT NULL,
                 value       REAL NOT NULL
             );
             -- Reading one channel over a time range is what every plot does.
             CREATE INDEX IF NOT EXISTS readings_by_channel
                 ON readings(channel_id, timestamp);",
        )?;

        self.connection.execute(
            "INSERT INTO daq (name, author) VALUES (?1, ?2)",
            rusqlite::params![header.info.name, header.info.author],
        )?;

        for device in header.devices.iter() {
            self.connection.execute(
                "INSERT INTO devices (name, description) VALUES (?1, ?2)",
                rusqlite::params![device.info.name, device.info.description],
            )?;
            let device_id = self.connection.last_insert_rowid();

            let mut ids: HashMap<String, i64> = HashMap::new();
            for channel in device.channels.iter() {
                self.connection.execute(
                    "INSERT INTO channels (device_id, name, unit, description)
                     VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![
                        device_id,
                        channel.name,
                        channel.unit,
                        channel.description
                    ],
                )?;
                ids.insert(channel.name.clone(), self.connection.last_insert_rowid());
            }
            self.channel_ids.insert(device.info.name.clone(), ids);
        }
        Ok(())
    }

    fn write_batch(&mut self, batch: &Batch) -> Result<()> {
        if batch.datapoints.is_empty() {
            return Ok(());
        }

        let channel_id = self
            .channel_ids
            .get(batch.device.as_str())
            .and_then(|channels| channels.get(batch.channel.as_str()))
            .copied()
            .ok_or_else(|| Error::UnknownChannel {
                device: batch.device.clone(),
                channel: batch.channel.clone(),
            })?;

        self.begin()?;
        let mut statement = self
            .connection
            .prepare_cached("INSERT INTO readings (channel_id, timestamp, value) VALUES (?1, ?2, ?3)")?;
        for datapoint in batch.datapoints.iter() {
            statement.execute(rusqlite::params![
                channel_id,
                datapoint.datetime.timestamp_micros(),
                datapoint.value
            ])?;
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        if self.uncommitted {
            self.connection.execute_batch("COMMIT")?;
            self.uncommitted = false;
        }
        Ok(())
    }
}

impl Drop for SqliteSink {
    /// Commit whatever is outstanding rather than rolling it back.
    ///
    /// Without this, a run that ends without a final flush would discard its
    /// last transaction, which is a surprising way to lose data.
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::ChannelInfo;
    use crate::daq::DaqInfo;
    use crate::datapoint::DataPoint;
    use crate::storage::DeviceHeader;
    use crate::device::DeviceInfo;

    fn header() -> DaqHeader {
        DaqHeader {
            info: DaqInfo { name: "Test".to_string(), author: "Nobody".to_string() },
            devices: vec![DeviceHeader {
                info: DeviceInfo {
                    name: "Serial test device".to_string(),
                    description: "-".to_string(),
                },
                channels: vec![ChannelInfo {
                    id: "1".to_string(),
                    name: "Pressure".to_string(),
                    unit: "Pa".to_string(),
                    description: "-".to_string(),
                }],
            }],
        }
    }

    fn batch(values: &[f64]) -> Batch {
        Batch {
            device: "Serial test device".to_string(),
            channel: "Pressure".to_string(),
            datapoints: values
                .iter()
                .map(|value| DataPoint { datetime: chrono::Utc::now(), value: *value })
                .collect(),
        }
    }

    fn sink_in(dir: &std::path::Path) -> SqliteSink {
        let mut sink = SqliteSink::new(&dir.join("results.db")).unwrap();
        sink.init(&header()).unwrap();
        sink
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("lumberdaq_{}", name));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn readings_land_against_the_right_channel() {
        let dir = temp_dir("sqlite_readings");
        let mut sink = sink_in(&dir);
        sink.write_batch(&batch(&[1.0, 2.0, 3.0])).unwrap();
        sink.flush().unwrap();

        let count: i64 = sink
            .connection
            .query_row(
                "SELECT COUNT(*) FROM readings r
                   JOIN channels c ON c.id = r.channel_id
                  WHERE c.name = 'Pressure'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 3);
    }

    /// A batch names its channel by string; anything not in the header has
    /// nowhere to go and must say so rather than being dropped.
    #[test]
    fn a_channel_missing_from_the_header_is_rejected() {
        let dir = temp_dir("sqlite_unknown");
        let mut sink = sink_in(&dir);
        let mut stray = batch(&[1.0]);
        stray.channel = "Not configured".to_string();
        assert!(matches!(
            sink.write_batch(&stray),
            Err(Error::UnknownChannel { .. })
        ));
    }

    /// Nothing is committed until flush, which is what keeps the insert rate up.
    #[test]
    fn writes_are_held_until_flushed() {
        let dir = temp_dir("sqlite_flush");
        let mut sink = sink_in(&dir);
        sink.write_batch(&batch(&[1.0, 2.0])).unwrap();
        assert!(sink.uncommitted);
        sink.flush().unwrap();
        assert!(!sink.uncommitted);
        // Flushing again with nothing outstanding is not an error.
        sink.flush().unwrap();
    }

    /// The description lives in the same file as the data, so there is no
    /// sidecar that can drift away from it.
    #[test]
    fn the_setup_is_recorded_alongside_the_readings() {
        let dir = temp_dir("sqlite_header");
        let sink = sink_in(&dir);
        let (name, unit): (String, String) = sink
            .connection
            .query_row("SELECT name, unit FROM channels", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap();
        assert_eq!(name, "Pressure");
        assert_eq!(unit, "Pa");
    }
}
