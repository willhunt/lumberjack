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

use raw::nidaqmx::NiDaqmx;
use std::ffi::CStr;

/// Where the driver lives.
///
/// The installer puts it in System32, which is on the search path, so the bare
/// name is enough on a normal installation. The full path is tried as well for
/// anywhere it is not.
#[cfg(windows)]
const DRIVER_CANDIDATES: &[&str] = &[
    "nicaiu.dll",
    r"C:\Windows\System32\nicaiu.dll",
];

/// NI-DAQmx for Linux installs the shared object under its own name.
#[cfg(not(windows))]
const DRIVER_CANDIDATES: &[&str] = &["libnidaqmx.so", "libnidaqmx.so.1"];

/// How much room to give the driver for a message or a list of names.
///
/// DAQmx reports what it needs when asked with a zero length, but every call
/// that matters here fits comfortably, and one round trip beats two.
const TEXT_BUFFER: usize = 4096;

#[derive(Debug)]
pub enum Error {
    /// The driver could not be loaded from anywhere we looked.
    DriverNotFound { tried: Vec<String> },
    /// The library loaded but does not export something we need, which means it
    /// is not the driver we think it is.
    SymbolMissing(&'static str),
    /// A call failed, with whatever the driver said about it. DAQmx explains
    /// itself in a sentence, which is worth passing on rather than a number.
    Failed {
        operation: &'static str,
        code: i32,
        message: String,
    },
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
pub struct Daqmx {
    api: NiDaqmx,
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
        Ok(Daqmx { api: api })
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
        self.check("DAQmxGetDevSerialNum", status)?;
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

    /// Run a call that fills a buffer with text.
    fn text(
        &self,
        operation: &'static str,
        call: impl FnOnce(*mut std::os::raw::c_char, u32) -> i32,
    ) -> Result<String> {
        let mut buffer = vec![0i8; TEXT_BUFFER];
        let status = call(buffer.as_mut_ptr(), TEXT_BUFFER as u32);
        self.check(operation, status)?;
        Ok(from_c(&buffer))
    }

    /// Turn a status code into an error, asking the driver what it meant.
    ///
    /// Negative is a failure and positive is a warning, which is not a reason
    /// to stop. Zero is fine.
    fn check(&self, operation: &'static str, status: i32) -> Result<()> {
        if status >= 0 {
            return Ok(());
        }
        Err(Error::Failed {
            operation: operation,
            code: status,
            message: self.last_message(),
        })
    }

    /// What the driver says went wrong, in words.
    ///
    /// This is the part of DAQmx that is nicer to work with than most drivers:
    /// it explains itself in a sentence rather than a number to look up.
    fn last_message(&self) -> String {
        let mut buffer = vec![0i8; TEXT_BUFFER];
        let status = unsafe {
            self.api.DAQmxGetExtendedErrorInfo(buffer.as_mut_ptr(), TEXT_BUFFER as u32)
        };
        match status {
            0 => from_c(&buffer),
            // Asking why it failed is not allowed to fail in turn.
            _ => "no description available".to_string(),
        }
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
