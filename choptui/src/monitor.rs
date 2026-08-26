//! What the display is told, and the sink that tells it.

use lumberdaq::config::DaqConfig;
use lumberdaq::session::DeviceEvent;
use lumberdaq::storage::{ Batch, DataSink };
use lumberdaq::Result;
use std::sync::mpsc::Sender;

/// One thing the display needs to know about.
///
/// Errors become text here rather than being passed on. The display only ever
/// prints them, and a string crossing to another thread asks nothing of the
/// error type, which is the same reason `DeviceEvent::Disconnected` already
/// carries a message rather than the error itself.
pub enum Update {
    Data {
        device: String,
        channel: String,
        value: f64,
        added: usize,
    },
    Connected {
        device: String,
    },
    Disconnected {
        device: String,
        cause: Option<String>,
    },
    Problem {
        device: String,
        message: String,
    },
}

impl Update {
    /// Whatever a run reports about a device, in the form the display wants.
    pub fn from_event(event: DeviceEvent) -> Update {
        match event {
            DeviceEvent::Connected { device } => Update::Connected { device: device },
            DeviceEvent::Disconnected { device, cause } => {
                Update::Disconnected { device: device, cause: cause }
            }
            DeviceEvent::Problem { device, error } => {
                Update::Problem { device: device, message: error.to_string() }
            }
        }
    }
}

/// A sink that draws nothing.
///
/// It summarises each batch and hands it to the thread that does the drawing,
/// so a slow redraw can never hold up a write to disk, and the display is fed
/// by exactly the data that was recorded rather than by a second route through
/// the library.
pub struct Monitor {
    updates: Sender<Update>,
}

impl Monitor {
    pub fn new(updates: Sender<Update>) -> Monitor {
        Monitor { updates: updates }
    }
}

impl DataSink for Monitor {
    fn init(&mut self, _config: &DaqConfig) -> Result<()> {
        Ok(())
    }

    fn write_batch(&mut self, batch: &Batch) -> Result<()> {
        // Only the newest matters on screen; the rest are already on their way
        // to disk by another sink.
        if let Some(latest) = batch.datapoints.last() {
            // Deliberately ignored. If the display has gone, the recording must
            // carry on: a monitor failing is no reason to stop a run, and this
            // is the whole of that policy.
            let _ = self.updates.send(Update::Data {
                device: batch.device.clone(),
                channel: batch.channel.clone(),
                value: latest.value,
                added: batch.datapoints.len(),
            });
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}
