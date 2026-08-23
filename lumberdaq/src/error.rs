// Best practise: https://www.youtube.com/watch?v=j-VQCYP7wyw

use thiserror::Error as ThisError;

pub type Result<T> = core::result::Result<T, Error>;

/// Everything that can go wrong in lumberdaq.
///
/// This used to be `Box<dyn std::error::Error>`, which was easy to produce and
/// impossible to inspect: by the time a failure reached a caller it was only
/// prose, so `Device::read` could not tell a device dropping off the bus from
/// one bad frame arriving on a perfectly healthy port.
///
/// Variants are being added a module at a time. Until a call site is converted
/// it produces `Other`, so `"...".into()` keeps working as before.
#[derive(Debug, ThisError)]
pub enum Error {
    // ---- Foreign errors ----------------------------------------------------
    // `#[from]` is what keeps `?` working: the question mark calls From::from,
    // so these convert on their own the way Box<dyn Error> used to.
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Csv(#[from] csv::Error),

    #[error(transparent)]
    SerialPort(#[from] serialport::Error),

    #[error(transparent)]
    ParseNumber(#[from] std::num::ParseFloatError),

    #[error(transparent)]
    ParseTimestamp(#[from] chrono::ParseError),

    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),

    // ---- Hardware ----------------------------------------------------------
    #[error("no hardware is configured for this device")]
    NoHardware,

    #[error("device on {port} is not connected")]
    NotConnected { port: String },

    #[error("cannot add a {expected} channel to this device")]
    WrongHardwareType { expected: String },

    // ---- Storage -----------------------------------------------------------
    #[error("channel '{channel}' of device '{device}' is not in the recorded setup")]
    UnknownChannel { device: String, channel: String },

    #[error("this results database uses schema version {found}, but this build writes version {expected}; record to a new file, or delete the old one")]
    DatabaseSchemaVersion { found: i32, expected: i32 },

    // ---- Serial framing and parsing ----------------------------------------
    #[error("frame pattern '{pattern}' for serial port {port} is not a valid regular expression")]
    InvalidFramePattern {
        pattern: String,
        port: String,
        #[source]
        source: regex::Error,
    },

    #[error("{bytes} bytes arrived on {port} with no complete frame; check the baud rate and the frame pattern")]
    NoFrameFound { port: String, bytes: usize },

    #[error("channel '{channel}' reads index {index}, but the frame has only {fields} fields: '{frame}'")]
    FrameTooShort {
        channel: String,
        index: i64,
        fields: usize,
        frame: String,
    },

    #[error("channel '{channel}' read '{field}' at index {index} of frame '{frame}', which is not a number")]
    FieldNotNumeric {
        channel: String,
        index: i64,
        field: String,
        frame: String,
    },

    #[error("channel '{channel}' has a negative index {index}")]
    NegativeChannelIndex { channel: String, index: i64 },

    // ---- Not yet converted -------------------------------------------------
    /// A failure that has not been given a variant of its own yet.
    ///
    /// This is scaffolding for the migration, not a permanent home. It should
    /// shrink to nothing as each module gets its own variants.
    #[error("{0}")]
    Other(String),
}

impl Error {
    /// Whether this means the device has gone away, rather than the data or the
    /// configuration being wrong.
    ///
    /// This is the distinction the old string errors could not express. A
    /// device that has genuinely dropped off should be reconnected; a frame
    /// that failed to parse should not, because closing and reopening a healthy
    /// port fixes nothing and loses whatever arrives meanwhile.
    ///
    /// Anything not listed here is treated as the device being fine, which is
    /// the safer default: a spurious reconnect is worse than a reported error.
    pub fn is_connection_lost(&self) -> bool {
        match self {
            Error::NotConnected { .. } => true,
            Error::NoHardware => true,
            // The port itself is unhappy: unplugged, or the OS handle is gone.
            Error::Io(_) => true,
            Error::SerialPort(_) => true,
            // Framing, parsing and configuration problems. The device is there;
            // what it sent, or what we asked for, is wrong.
            _ => false,
        }
    }
}

// These two are why every existing `"...".into()` and `format!(...).into()`
// still compiles. They go away with the `Other` variant.
impl From<String> for Error {
    fn from(message: String) -> Error {
        Error::Other(message)
    }
}

impl From<&str> for Error {
    fn from(message: &str) -> Error {
        Error::Other(message.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Box<dyn std::error::Error> is not Send, so it could never have been sent
    /// back from a per-device thread. A concrete enum is Send and Sync as long
    /// as its fields are; this fails to compile the moment a variant holds
    /// something that is not, which is exactly when we would want to know.
    #[test]
    fn errors_can_cross_a_thread_boundary() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Error>();
    }

    /// The escape hatch keeps existing call sites compiling unchanged.
    #[test]
    fn strings_still_convert_for_call_sites_not_yet_converted() {
        let from_str: Error = "something went wrong".into();
        let from_string: Error = format!("device {} is missing", "COM3").into();
        assert_eq!(from_str.to_string(), "something went wrong");
        assert_eq!(from_string.to_string(), "device COM3 is missing");
    }

    /// Foreign errors keep their own message rather than being restated.
    #[test]
    fn a_wrapped_error_displays_as_itself() {
        let io = std::io::Error::new(std::io::ErrorKind::NotFound, "no such port");
        let error: Error = io.into();
        assert_eq!(error.to_string(), "no such port");
    }
}
