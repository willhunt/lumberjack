//! The log, kept on disk as well as on screen.
//!
//! The pane at the bottom of the window holds the last couple of hundred lines
//! and loses all of them when the window closes. That is the right thing while
//! somebody is watching; it is no use at all afterwards, which is when a log is
//! usually wanted — "it stopped recording overnight" is a question about lines
//! nobody was there to read.
//!
//! Beside the settings, because it belongs to this person and this machine
//! rather than to any one project.

use chrono::{DateTime, Local, Utc};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

/// How large the log may grow before the previous one is set aside.
///
/// Two files, bounded: a session's worth of lines is a few kilobytes, so a
/// megabyte is many sessions of history, and nothing here can quietly fill a
/// disk.
const ROLL_AT: u64 = 1024 * 1024;

/// The log file, once it has been opened.
///
/// Holds the handle rather than reopening per line: a line is written for
/// every device event and every save, and reopening a file for each would be
/// paying for the wrong thing.
pub(crate) struct Logbook {
    file: Option<File>,
}

impl Logbook {
    /// Open the log for appending, rolling the old one aside if it has grown.
    ///
    /// Never fails. A program that will not start because it could not open
    /// its log has its priorities backwards — but it does say so, once, in the
    /// log on screen, because silently keeping no record is how you discover
    /// months later that there was never anything to send.
    pub(crate) fn open() -> (Logbook, Option<String>) {
        let Some(path) = path() else {
            return (Logbook { file: None }, Some("no folder to keep a log in".to_string()));
        };

        Logbook::at(path)
    }

    /// The same, at a path of the caller's choosing.
    ///
    /// Split from `open` so the rolling and the appending can be tested
    /// somewhere harmless. A test that exercised the real path would be
    /// writing into whoever ran it, and would be reading back whatever they
    /// had done that day.
    pub(crate) fn at(path: PathBuf) -> (Logbook, Option<String>) {
        if let Some(parent) = path.parent() {
            if let Err(problem) = std::fs::create_dir_all(parent) {
                return (Logbook { file: None }, Some(format!("no log: {}", problem)));
            }
        }

        // Rolled before opening, so the check is against what the last session
        // left rather than against a file this one is already writing to.
        if std::fs::metadata(&path).map(|found| found.len() > ROLL_AT).unwrap_or(false) {
            let _ = std::fs::rename(&path, path.with_extension("log.previous"));
        }

        match OpenOptions::new().create(true).append(true).open(&path) {
            Ok(file) => {
                let mut logbook = Logbook { file: Some(file) };
                // A line to say which run of which build the lines below came
                // from, since the file outlives the session.
                logbook.write(
                    Utc::now(),
                    &format!("--- lumbergui {} started ---", env!("CARGO_PKG_VERSION")),
                );
                (logbook, None)
            }
            Err(problem) => {
                (Logbook { file: None }, Some(format!("no log at {}: {}", path.display(), problem)))
            }
        }
    }

    /// Write one line, and make sure it has actually landed.
    ///
    /// Flushed rather than buffered, because the lines worth having are the
    /// ones written just before something went wrong, and a buffer is exactly
    /// what loses those. The log is a handful of lines a minute, so the cost
    /// of a write per line is not worth avoiding.
    pub(crate) fn write(&mut self, at: DateTime<Utc>, text: &str) {
        let Some(file) = self.file.as_mut() else { return };

        // Local time, as the pane shows it, so a line quoted from one matches
        // a line found in the other.
        let stamp = at.with_timezone(&Local).format("%Y-%m-%d %H:%M:%S");

        // Nothing to be done if this fails, and nowhere to report it: the
        // report would be another line in the log that is not being written.
        let _ = writeln!(file, "{} - {}", stamp, text);
        let _ = file.flush();
    }
}

/// Where the log lives: beside the settings file.
fn path() -> Option<PathBuf> {
    Some(crate::settings::config_dir()?.join("lumberjack.log"))
}

/// Write panics to the log as well as to a console nobody is watching.
///
/// A panic in the interface closes the window and prints to a stderr that a
/// program started from Explorer does not have, so what somebody sees is the
/// application vanishing. That is the least diagnosable failure there is, and
/// it is the one most worth a record.
///
/// The old hook is kept and called afterwards, so running from a terminal
/// still prints what it always did.
pub(crate) fn catch_panics() {
    let previous = std::panic::take_hook();

    std::panic::set_hook(Box::new(move |info| {
        // A handle of its own rather than the interface's. Whatever is panicking
        // may be holding the interface's, and asking a dying thread for
        // something it owns is how a crash becomes a hang.
        if let Some(path) = path() {
            if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
                let stamp = Local::now().format("%Y-%m-%d %H:%M:%S");
                let at = match info.location() {
                    Some(location) => format!("{}:{}", location.file(), location.line()),
                    None => "an unknown place".to_string(),
                };

                let _ = writeln!(file, "{} - PANIC at {}: {}", stamp, at, describe(info.payload()));
                // Forced, rather than left to `RUST_BACKTRACE`: nobody setting
                // an environment variable was ever the person this is for.
                let _ = writeln!(file, "{}", std::backtrace::Backtrace::force_capture());
                let _ = file.flush();
            }
        }

        previous(info);
    }));
}

/// What a panic was about, as far as its payload will say.
///
/// A panic carries `Any`, which in practice is the message from `panic!` as
/// either a literal or a formatted string. Anything else is possible and says
/// nothing useful, so it is named rather than guessed at.
fn describe(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        return (*message).to_string();
    }
    if let Some(message) = payload.downcast_ref::<String>() {
        return message.clone();
    }
    "a panic carrying no message".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("lumbergui_log_{}.log", name));
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("log.previous"));
        path
    }

    #[test]
    fn a_session_writes_its_lines_and_says_which_build_wrote_them() {
        let path = scratch("writes");
        let (mut logbook, complaint) = Logbook::at(path.clone());

        assert!(complaint.is_none(), "{:?}", complaint);
        logbook.write(Utc::now(), "a device was connected");

        let written = std::fs::read_to_string(&path).expect("the log should be there");
        assert!(written.contains("lumbergui"), "{}", written);
        assert!(written.contains("a device was connected"), "{}", written);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_later_session_adds_to_the_last_one() {
        // Appended rather than replaced: the session worth reading is often
        // the one before the one that went wrong.
        let path = scratch("appends");

        let (mut first, _) = Logbook::at(path.clone());
        first.write(Utc::now(), "the first session");
        drop(first);

        let (mut second, _) = Logbook::at(path.clone());
        second.write(Utc::now(), "the second session");

        let written = std::fs::read_to_string(&path).expect("the log should be there");
        assert!(written.contains("the first session"), "{}", written);
        assert!(written.contains("the second session"), "{}", written);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_panic_message_is_read_out_of_its_payload() {
        // The two shapes a panic actually carries: a literal, and a formatted
        // string. Anything else is possible and worth saying so about.
        assert_eq!(describe(&"the plot was not there"), "the plot was not there");
        assert_eq!(describe(&format!("channel {} is missing", 4)), "channel 4 is missing");
        assert_eq!(describe(&7u32), "a panic carrying no message");
    }

    #[test]
    fn a_log_that_has_grown_is_set_aside_rather_than_left_to_grow() {
        let path = scratch("rolls");
        let previous = path.with_extension("log.previous");

        std::fs::write(&path, "x".repeat(ROLL_AT as usize + 1)).expect("a big log");

        let (mut logbook, _) = Logbook::at(path.clone());
        logbook.write(Utc::now(), "after the roll");

        let kept = std::fs::read_to_string(&previous).expect("the old one should be kept");
        assert_eq!(kept.len() as u64, ROLL_AT + 1);

        let fresh = std::fs::read_to_string(&path).expect("a new one should be started");
        assert!(fresh.contains("after the roll"), "{}", fresh);
        assert!(fresh.len() < 500, "the new log should not hold the old one");

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&previous);
    }
}
