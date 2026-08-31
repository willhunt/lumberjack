//! Safe wrappers over the National Instruments DAQmx driver.
//!
//! Enough of it to read analog input from a USB-6001. DAQmx is vast — the
//! header declares over three thousand functions, most of them property
//! accessors — so the bindings cover only what is called here, and grow when
//! something needs them.
//!
//! Nothing links against the driver. It is looked up at run time, so this
//! builds and loads on a machine with no NI software installed and fails when a
//! device is opened, saying what it looked for. Only somebody actually reading
//! NI hardware needs the DAQmx runtime.

pub mod raw;

use raw::nidaqmx::{ NiDaqmx, TaskHandle };
use std::ffi::CStr;
use std::sync::Arc;

/// Where the driver lives.
///
/// The installer puts it in System32, which is on the search path, so the bare
/// name is enough on a normal installation. The full path is tried as well for
/// anywhere it is not.
#[cfg(windows)]
const DRIVER_CANDIDATES: &[&str] = &["nicaiu.dll", r"C:\Windows\System32\nicaiu.dll"];

/// NI-DAQmx for Linux installs the shared object under its own name.
#[cfg(not(windows))]
const DRIVER_CANDIDATES: &[&str] = &["libnidaqmx.so", "libnidaqmx.so.1"];

/// How much room to give the driver for a message or a list of names.
///
/// DAQmx reports what it needs when asked with a zero length, but every call
/// that matters here fits comfortably, and one round trip beats two.
const TEXT_BUFFER: usize = 4096;

/// How long to wait for a reading before giving up.
const READ_TIMEOUT_SECONDS: f64 = 10.0;

#[derive(Debug)]
pub enum Error {
    /// The driver could not be loaded from anywhere we looked.
    DriverNotFound { tried: Vec<String> },
    /// The library loaded but does not export something we need, which means it
    /// is not the driver we think it is.
    SymbolMissing(&'static str),
    /// A call failed, with whatever the driver said about it. DAQmx explains
    /// itself in a sentence, which is worth passing on rather than a number.
    Failed { operation: &'static str, code: i32, message: String },
}

impl std::fmt::Display for Error {
    fn fmt(&self, out: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::DriverNotFound { tried } => write!(
                out,
                "could not load the NI-DAQmx driver. Tried: {}. Reading NI hardware needs the \
                 NI-DAQmx runtime installed; nothing else here does",
                tried.join(", ")
            ),
            Error::SymbolMissing(symbol) => write!(
                out,
                "the NI-DAQmx driver loaded but does not export {}, so it is not the driver this \
                 expects",
                symbol
            ),
            Error::Failed { operation, code, message } => {
                write!(out, "{} failed ({}): {}", operation, code, message)
            }
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// How an analog input is wired up.
///
/// Which of these a channel will accept depends on the model and on the channel
/// number, so it is asked of the driver rather than worked out from a rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Terminal {
    /// Measured against the device's own ground.
    SingleEnded,
    /// Measured between a pair of inputs. NI pairs a channel with the one four
    /// above it, so on an eight input device only the first four can start a
    /// pair — but that is the documented rule, not a checked one, which is what
    /// `probe_terminals` is for.
    Differential,
}

impl Terminal {
    fn code(self) -> i32 {
        match self {
            Terminal::SingleEnded => raw::nidaqmx::DAQmx_Val_RSE as i32,
            Terminal::Differential => raw::nidaqmx::DAQmx_Val_Diff as i32,
        }
    }
}

/// How far apart the two halves of a differential pair are.
///
/// NI pairs a channel with the one four above it, unlike Pico which pairs with
/// the one immediately above. Confirmed against a USB-6001 by
/// `examples/probe_terminals.rs`: ai0 to ai3 accept differential and ai4 to ai7
/// are refused, the driver naming `DAQmx_Val_RSE` as the only value it would
/// take for those.
pub const DIFFERENTIAL_OFFSET: u32 = 4;

/// Which input a differential measurement is taken against.
pub fn differential_partner(channel: u32) -> u32 {
    channel + DIFFERENTIAL_OFFSET
}

/// Whether a channel can start a differential pair, on a device with `inputs`
/// analog inputs.
///
/// Worth answering without the hardware, so that a setup can be checked from a
/// desk rather than finding out on the first reading of a run.
pub fn can_be_differential(channel: u32, inputs: usize) -> bool {
    (differential_partner(channel) as usize) < inputs
}

/// Fail unless the loaded library exports everything called here.
///
/// Without this a missing symbol is a panic at the point of use, long after the
/// thing that could have explained it.
macro_rules! require_symbols {
    ($api:expr, $($symbol:ident),+ $(,)?) => {
        $(
            if $api.$symbol.is_err() {
                return Err(Error::SymbolMissing(stringify!($symbol)));
            }
        )+
    };
}

/// A loaded DAQmx driver.
///
/// Holds no device. DAQmx addresses hardware by name, as in `Dev1/ai0`, so
/// there is nothing to open and nothing to close: what a device is called is
/// decided in NI MAX, and this asks the driver which names exist.
///
/// The driver is behind an `Arc` so that a [`Task`] can keep it alive on its
/// own. A task borrowing the driver would work, but then nothing could hold
/// both — a struct owning a `Daqmx` and a task borrowed from it is
/// self-referential, which Rust will not have. Sharing ownership is the way out
/// of that, and it costs one atomic increment per task.
pub struct Daqmx {
    api: Arc<NiDaqmx>,
}

impl Daqmx {
    /// Load the driver from wherever it is installed.
    pub fn load() -> Result<Daqmx> {
        let mut tried: Vec<String> = Vec::new();
        for candidate in DRIVER_CANDIDATES {
            // SAFETY: loading a library runs its initialisation code, which we
            // are trusting NI's driver to do sanely. There is no way to load a
            // C library without this.
            match unsafe { NiDaqmx::new(candidate) } {
                Ok(api) => return Daqmx::with(api),
                Err(_) => tried.push((*candidate).to_string()),
            }
        }
        Err(Error::DriverNotFound { tried: tried })
    }

    fn with(api: NiDaqmx) -> Result<Daqmx> {
        require_symbols!(
            api,
            DAQmxGetSysDevNames,
            DAQmxGetDevProductType,
            DAQmxGetDevSerialNum,
            DAQmxGetDevAIPhysicalChans,
            DAQmxGetExtendedErrorInfo,
            DAQmxCreateTask,
            DAQmxClearTask,
            DAQmxStartTask,
            DAQmxStopTask,
            DAQmxCreateAIVoltageChan,
            DAQmxCfgSampClkTiming,
            DAQmxReadAnalogF64,
            DAQmxReadAnalogScalarF64,
        );
        Ok(Daqmx { api: Arc::new(api) })
    }

    /// The devices NI MAX knows about, real or simulated.
    ///
    /// A simulated device is indistinguishable here, which is the point of one:
    /// everything can be built and exercised with nothing plugged in.
    pub fn devices(&self) -> Result<Vec<String>> {
        let names = self.text("DAQmxGetSysDevNames", |buffer, size| unsafe {
            self.api.DAQmxGetSysDevNames(buffer, size)
        })?;
        Ok(split_names(&names))
    }

    /// What model a device is, as the driver reports it.
    pub fn product_type(&self, device: &str) -> Result<String> {
        let name = c_string(device);
        self.text("DAQmxGetDevProductType", |buffer, size| unsafe {
            self.api.DAQmxGetDevProductType(name.as_ptr(), buffer, size)
        })
    }

    /// The serial number, or zero for a simulated device.
    pub fn serial_number(&self, device: &str) -> Result<u32> {
        let name = c_string(device);
        let mut serial: u32 = 0;
        let status = unsafe { self.api.DAQmxGetDevSerialNum(name.as_ptr(), &mut serial) };
        check(&self.api, "DAQmxGetDevSerialNum", status)?;
        Ok(serial)
    }

    /// Every analog input a device has, as `Dev1/ai0` and so on.
    ///
    /// Asked of the driver rather than assumed from the model, since that is
    /// the list a channel has to be named from.
    pub fn analog_inputs(&self, device: &str) -> Result<Vec<String>> {
        let name = c_string(device);
        let channels = self.text("DAQmxGetDevAIPhysicalChans", |buffer, size| unsafe {
            self.api.DAQmxGetDevAIPhysicalChans(name.as_ptr(), buffer, size)
        })?;
        Ok(split_names(&channels))
    }

    /// Start a task, which is how DAQmx groups the channels read together.
    ///
    /// The name is DAQmx's own label for it and may be empty, which asks the
    /// driver to make one up.
    pub fn task(&self, name: &str) -> Result<Task> {
        let name = c_string(name);
        let mut handle: TaskHandle = std::ptr::null_mut();
        let status = unsafe { self.api.DAQmxCreateTask(name.as_ptr(), &mut handle) };
        check(&self.api, "DAQmxCreateTask", status)?;
        Ok(Task { api: Arc::clone(&self.api), handle: handle, channels: 0, running: false })
    }

    /// Run a call that fills a buffer with text.
    fn text(
        &self,
        operation: &'static str,
        call: impl FnOnce(*mut std::os::raw::c_char, u32) -> i32,
    ) -> Result<String> {
        let mut buffer = vec![0i8; TEXT_BUFFER];
        let status = call(buffer.as_mut_ptr(), TEXT_BUFFER as u32);
        check(&self.api, operation, status)?;
        Ok(from_c(&buffer))
    }
}

/// A group of channels read together.
///
/// DAQmx does nothing per channel: channels are added to a task, and the task
/// is what starts, reads and stops. One reading of a task gives one value for
/// every channel on it, which is also why they all share a timestamp.
pub struct Task {
    api: Arc<NiDaqmx>,
    handle: TaskHandle,
    channels: usize,
    running: bool,
}

// SAFETY: a task handle is an opaque pointer into the driver, which Rust
// therefore assumes is not safe to move between threads. DAQmx is documented as
// safe to use from any thread so long as one task is not used from two at once,
// and Rust's ownership rules already guarantee that: reading takes `&mut self`,
// so only one thread can be in a task at a time. lumberdaq reads every device on
// its own thread, so without this a task could not be a backend at all.
unsafe impl Send for Task {}

impl Task {
    /// Add one analog input, measured as volts.
    ///
    /// `range` is the span expected, which the driver uses to pick a gain. It
    /// is asked for rather than assumed because a device offers several and the
    /// nearest one that fits is the one worth having.
    pub fn add_voltage_input(
        &mut self,
        channel: &str,
        terminal: Terminal,
        range: (f64, f64),
    ) -> Result<()> {
        let channel = c_string(channel);
        let unnamed = c_string("");
        let status = unsafe {
            self.api.DAQmxCreateAIVoltageChan(
                self.handle,
                channel.as_ptr(),
                // No name of our own: the physical channel is what a reading
                // comes back in the order of, and a second name for it is one
                // more thing to keep in step.
                unnamed.as_ptr(),
                terminal.code(),
                range.0,
                range.1,
                raw::nidaqmx::DAQmx_Val_Volts as i32,
                // A custom scale would be DAQmx doing the conversion. Scaling
                // belongs to the channel config, where every backend shares it.
                std::ptr::null(),
            )
        };
        check(&self.api, "DAQmxCreateAIVoltageChan", status)?;
        self.channels += 1;
        Ok(())
    }

    /// How many channels have been added.
    pub fn channels(&self) -> usize {
        self.channels
    }

    /// Commit the task and begin acquiring.
    ///
    /// Worth doing before the first read even though reading would start it:
    /// a configuration the hardware will not accept is refused here, at setup,
    /// rather than on the first reading of a run already under way.
    pub fn start(&mut self) -> Result<()> {
        let status = unsafe { self.api.DAQmxStartTask(self.handle) };
        check(&self.api, "DAQmxStartTask", status)?;
        self.running = true;
        Ok(())
    }

    pub fn stop(&mut self) -> Result<()> {
        let status = unsafe { self.api.DAQmxStopTask(self.handle) };
        check(&self.api, "DAQmxStopTask", status)?;
        self.running = false;
        Ok(())
    }

    /// Take one reading from every channel, in the order they were added.
    ///
    /// With no sample clock configured this is DAQmx's on demand mode: the
    /// device converts when asked, which is what a polled backend wants.
    pub fn read_one(&mut self) -> Result<Vec<f64>> {
        if !self.running {
            self.start()?;
        }
        let mut values = vec![0f64; self.channels];
        let mut read: i32 = 0;
        let status = unsafe {
            self.api.DAQmxReadAnalogF64(
                self.handle,
                1,
                READ_TIMEOUT_SECONDS,
                // One value per channel, so either grouping gives the same
                // thing; by channel is the order the channels were added in.
                raw::nidaqmx::DAQmx_Val_GroupByChannel,
                values.as_mut_ptr(),
                values.len() as u32,
                &mut read,
                std::ptr::null_mut(),
            )
        };
        check(&self.api, "DAQmxReadAnalogF64", status)?;
        // One sample per channel was asked for with a timeout, so DAQmx either
        // gives that or fails. Anything else would be a row of zeros
        // indistinguishable from real readings, so say so rather than pass it
        // off as data.
        if read != 1 {
            return Err(Error::Failed {
                operation: "DAQmxReadAnalogF64",
                code: 0,
                message: format!("asked for one sample per channel and got {}", read),
            });
        }
        Ok(values)
    }
}

impl Drop for Task {
    fn drop(&mut self) {
        // Clearing stops it as well, and there is nothing useful to do with a
        // failure here: the task is going either way.
        unsafe { self.api.DAQmxClearTask(self.handle) };
    }
}

/// Turn a status code into an error, asking the driver what it meant.
///
/// Negative is a failure and positive is a warning, which is not a reason to
/// stop. Zero is fine.
fn check(api: &NiDaqmx, operation: &'static str, status: i32) -> Result<()> {
    if status >= 0 {
        return Ok(());
    }
    Err(Error::Failed { operation: operation, code: status, message: last_message(api) })
}

/// What the driver says went wrong, in words.
///
/// This is the part of DAQmx that is nicer to work with than most drivers: it
/// explains itself in a sentence rather than a number to look up.
fn last_message(api: &NiDaqmx) -> String {
    let mut buffer = vec![0i8; TEXT_BUFFER];
    let status =
        unsafe { api.DAQmxGetExtendedErrorInfo(buffer.as_mut_ptr(), TEXT_BUFFER as u32) };
    match status {
        0 => from_c(&buffer),
        // Asking why it failed is not allowed to fail in turn.
        _ => "no description available".to_string(),
    }
}

/// A comma separated list from the driver, with the spaces it puts in.
fn split_names(list: &str) -> Vec<String> {
    list.split(',')
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .collect()
}

fn c_string(text: &str) -> std::ffi::CString {
    // A device name comes from the driver or from a config file, and neither
    // has any business holding a nul. If one does, stopping at it is closer to
    // right than refusing to look the device up at all.
    std::ffi::CString::new(text).unwrap_or_else(|_| std::ffi::CString::new("").unwrap())
}

fn from_c(buffer: &[i8]) -> String {
    // SAFETY: the driver nul terminates what it writes, and the buffer was
    // zeroed, so there is a nul either way.
    let text = unsafe { CStr::from_ptr(buffer.as_ptr()) };
    text.to_string_lossy().trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_differential_pair_is_four_apart() {
        // Not one apart, which is how a Pico pairs them. The config surface
        // looks the same for both and the arithmetic cannot be shared.
        assert_eq!(differential_partner(0), 4);
        assert_eq!(differential_partner(3), 7);
    }

    #[test]
    fn only_the_lower_half_of_the_inputs_can_start_a_pair() {
        // Confirmed against a USB-6001: ai0 to ai3 accepted, ai4 to ai7 refused.
        for channel in 0..4 {
            assert!(can_be_differential(channel, 8), "ai{} should pair", channel);
        }
        for channel in 4..8 {
            assert!(!can_be_differential(channel, 8), "ai{} has no partner", channel);
        }
    }

    #[test]
    fn a_device_with_more_inputs_can_pair_more_of_them() {
        // The rule is about having a partner, not about the number four.
        assert!(can_be_differential(7, 16));
        assert!(!can_be_differential(12, 16));
    }
}
