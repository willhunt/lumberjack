//! ADC-20 and ADC-24 high resolution data loggers.
//!
//! A safe layer over `src/raw/hrdl.rs`. The driver is opened at runtime rather
//! than linked, so a machine with no Pico software installed still builds and
//! runs; it fails here, with something worth reading.

use crate::raw::hrdl::PicoHrdl;
use std::fmt;

/// Where to look for the driver, in order.
///
/// The bare name first, which resolves if the dll sits beside the executable or
/// on PATH: that is how a packaged install should ship it. The SDK location is
/// the fallback for a development machine, where PicoSDK puts the dll somewhere
/// nothing points at.
#[cfg(windows)]
const DRIVER_CANDIDATES: &[&str] = &[
    "picohrdl.dll",
    r"C:\Program Files\Pico Technology\SDK\lib\picohrdl.dll",
];
#[cfg(not(windows))]
const DRIVER_CANDIDATES: &[&str] = &["libpicohrdl.so", "/opt/picoscope/lib/libpicohrdl.so"];

/// The driver reports every failure as 0 and every success as 1.
const FAILED: i16 = 0;

/// Analog inputs are numbered from 1. Channel 0 is the digital block.
pub const MIN_CHANNEL: u16 = 1;
pub const MAX_CHANNEL: u16 = 16;

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

/// Full scale input range. Values are the header's, not invented here.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoltageRange {
    MilliVolts2500,
    MilliVolts1250,
    MilliVolts625,
    MilliVolts313,
    MilliVolts156,
    MilliVolts78,
    MilliVolts39,
}

impl VoltageRange {
    fn as_raw(self) -> i16 {
        match self {
            VoltageRange::MilliVolts2500 => 0,
            VoltageRange::MilliVolts1250 => 1,
            VoltageRange::MilliVolts625 => 2,
            VoltageRange::MilliVolts313 => 3,
            VoltageRange::MilliVolts156 => 4,
            VoltageRange::MilliVolts78 => 5,
            VoltageRange::MilliVolts39 => 6,
        }
    }

    /// Full scale in millivolts. The 313, 156, 78 and 39 names are rounded in
    /// the header; the true scale halves at each step down from 2500.
    pub fn millivolts(self) -> f64 {
        2500.0 / f64::from(1u32 << (self.as_raw() as u32))
    }
}

/// How long the converter integrates for, per channel.
///
/// This is the floor on how fast a unit can be read: a scan costs at least this
/// much for every enabled channel, so eight channels at the fastest setting is
/// already most of half a second.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversionTime {
    #[default]
    Ms60,
    Ms100,
    Ms180,
    Ms340,
    Ms660,
}

impl ConversionTime {
    fn as_raw(self) -> i16 {
        match self {
            ConversionTime::Ms60 => 0,
            ConversionTime::Ms100 => 1,
            ConversionTime::Ms180 => 2,
            ConversionTime::Ms340 => 3,
            ConversionTime::Ms660 => 4,
        }
    }

    pub fn millis(self) -> u64 {
        match self {
            ConversionTime::Ms60 => 60,
            ConversionTime::Ms100 => 100,
            ConversionTime::Ms180 => 180,
            ConversionTime::Ms340 => 340,
            ConversionTime::Ms660 => 660,
        }
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// `HRDL_ERROR` from the driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverError {
    Ok,
    KernelDriver,
    NotFound,
    ConfigFail,
    OsNotSupported,
    MaxDevices,
    Unknown(i32),
}

impl DriverError {
    fn from_code(code: i32) -> DriverError {
        match code {
            0 => DriverError::Ok,
            1 => DriverError::KernelDriver,
            2 => DriverError::NotFound,
            3 => DriverError::ConfigFail,
            4 => DriverError::OsNotSupported,
            5 => DriverError::MaxDevices,
            other => DriverError::Unknown(other),
        }
    }
}

impl fmt::Display for DriverError {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        let text = match self {
            DriverError::Ok => "no error reported",
            DriverError::KernelDriver => "the kernel driver is missing or the wrong version",
            DriverError::NotFound => "no unit found",
            DriverError::ConfigFail => "the unit could not be configured",
            DriverError::OsNotSupported => "this operating system is not supported",
            DriverError::MaxDevices => "too many units are already open",
            DriverError::Unknown(code) => return write!(formatter, "driver error {}", code),
        };
        formatter.write_str(text)
    }
}

/// `HRDL_SETTINGS`, which says which argument the driver objected to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsError {
    ConversionTimeOutOfRange,
    SampleIntervalOutOfRange,
    ConversionTimeTooSlow,
    ChannelNotAvailable,
    InvalidChannel,
    InvalidVoltageRange,
    InvalidParameter,
    ConversionInProgress,
    CommunicationFailed,
    Ok,
    Unknown(i32),
}

impl SettingsError {
    fn from_code(code: i32) -> SettingsError {
        match code {
            0 => SettingsError::ConversionTimeOutOfRange,
            1 => SettingsError::SampleIntervalOutOfRange,
            2 => SettingsError::ConversionTimeTooSlow,
            3 => SettingsError::ChannelNotAvailable,
            4 => SettingsError::InvalidChannel,
            5 => SettingsError::InvalidVoltageRange,
            6 => SettingsError::InvalidParameter,
            7 => SettingsError::ConversionInProgress,
            8 => SettingsError::CommunicationFailed,
            9 => SettingsError::Ok,
            other => SettingsError::Unknown(other),
        }
    }
}

impl fmt::Display for SettingsError {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        let text = match self {
            SettingsError::ConversionTimeOutOfRange => "conversion time out of range",
            SettingsError::SampleIntervalOutOfRange => "sample interval out of range",
            SettingsError::ConversionTimeTooSlow => {
                "conversion time too slow for the sample interval"
            }
            SettingsError::ChannelNotAvailable => "channel not available on this unit",
            SettingsError::InvalidChannel => "invalid channel",
            SettingsError::InvalidVoltageRange => "invalid voltage range",
            SettingsError::InvalidParameter => "invalid parameter",
            SettingsError::ConversionInProgress => "a conversion is already in progress",
            SettingsError::CommunicationFailed => "communication with the unit failed",
            SettingsError::Ok => "settings accepted",
            SettingsError::Unknown(code) => return write!(formatter, "settings error {}", code),
        };
        formatter.write_str(text)
    }
}

#[derive(Debug)]
pub enum Error {
    /// The driver could not be loaded from anywhere we looked.
    DriverNotFound { tried: Vec<String> },
    /// The library loaded but does not export something we need, which means it
    /// is not the driver we think it is.
    SymbolMissing(&'static str),
    /// The driver loaded and reported no unit attached.
    NoUnitFound,
    /// A call failed. The driver is asked afterwards what it objected to.
    Failed {
        operation: &'static str,
        driver: DriverError,
        settings: SettingsError,
    },
    ChannelOutOfRange(u16),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::DriverNotFound { tried } => write!(
                formatter,
                "could not load the PicoLog high resolution driver. Tried: {}. \
                 Install PicoSDK, or put the library beside the executable.",
                tried.join(", ")
            ),
            Error::SymbolMissing(name) => write!(
                formatter,
                "the driver that loaded does not export {}, so it is not the expected one",
                name
            ),
            Error::NoUnitFound => {
                formatter.write_str("the driver loaded but found no unit attached")
            }
            Error::Failed { operation, driver, settings } => {
                write!(formatter, "{} failed: {} ({})", operation, settings, driver)
            }
            Error::ChannelOutOfRange(channel) => write!(
                formatter,
                "channel {} is outside the {}..={} the unit provides",
                channel, MIN_CHANNEL, MAX_CHANNEL
            ),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

// ---------------------------------------------------------------------------
// The unit
// ---------------------------------------------------------------------------

/// One reading, as the converter produced it.
#[derive(Debug, Clone, Copy)]
pub struct Reading {
    /// Raw converter counts. Kept rather than only the scaled value, since it
    /// is what the hardware actually measured.
    pub counts: i32,
    /// The input was outside the selected range, so the value is clipped.
    pub overflow: bool,
}

/// An open ADC-20 or ADC-24.
///
/// Closing is handled by `Drop`, so a unit cannot be left open by an early
/// return or a panic.
pub struct Hrdl {
    api: PicoHrdl,
    handle: i16,
}

/// Refuse to build a `Hrdl` around a library missing anything we call, so a
/// missing symbol is an error at open rather than a panic much later. The
/// generated wrappers `expect()` on the symbol, which would take the process
/// down in the middle of a run.
macro_rules! require_symbols {
    ($api:expr, $($symbol:ident),+ $(,)?) => {
        $(
            if $api.$symbol.is_err() {
                return Err(Error::SymbolMissing(stringify!($symbol)));
            }
        )+
    };
}

impl Hrdl {
    /// Load the driver and open the first attached unit.
    pub fn open() -> Result<Hrdl> {
        let mut tried: Vec<String> = Vec::new();
        for candidate in DRIVER_CANDIDATES {
            // SAFETY: loading a library runs its initialisation code, which we
            // are trusting Pico's driver to do sanely. There is no way to load
            // a C library without this.
            match unsafe { PicoHrdl::new(candidate) } {
                Ok(api) => return Hrdl::open_with(api),
                Err(_) => tried.push((*candidate).to_string()),
            }
        }
        Err(Error::DriverNotFound { tried: tried })
    }

    fn open_with(api: PicoHrdl) -> Result<Hrdl> {
        require_symbols!(
            api,
            HRDLOpenUnit,
            HRDLCloseUnit,
            HRDLGetUnitInfo,
            HRDLSetAnalogInChannel,
            HRDLSetMains,
            HRDLGetMinMaxAdcCounts,
            HRDLGetSingleValue,
        );

        // SAFETY: takes no arguments and returns a handle by value.
        let handle = unsafe { api.HRDLOpenUnit() };
        if handle <= 0 {
            return Err(Error::NoUnitFound);
        }
        Ok(Hrdl { api: api, handle: handle })
    }

    /// A line of the unit's description, such as its serial or driver version.
    pub fn info(&self, line: Info) -> Result<String> {
        match self.info_text(line) {
            Some(text) => Ok(text),
            None => Err(self.failure("HRDLGetUnitInfo")),
        }
    }

    /// Reject mains hum at 50Hz or 60Hz.
    pub fn set_mains_rejection(&mut self, sixty_hertz: bool) -> Result<()> {
        // SAFETY: two integers by value.
        let result = unsafe { self.api.HRDLSetMains(self.handle, i16::from(sixty_hertz)) };
        if result == FAILED {
            return Err(self.failure("HRDLSetMains"));
        }
        Ok(())
    }

    /// Enable an analog input.
    ///
    /// In differential mode a channel is paired with the one above it, so only
    /// the odd numbered channels can be enabled.
    pub fn enable_channel(
        &mut self,
        channel: u16,
        range: VoltageRange,
        single_ended: bool,
    ) -> Result<()> {
        check_channel(channel)?;
        // SAFETY: five integers by value.
        let result = unsafe {
            self.api.HRDLSetAnalogInChannel(
                self.handle,
                channel as i16,
                1,
                range.as_raw(),
                i16::from(single_ended),
            )
        };
        if result == FAILED {
            return Err(self.failure("HRDLSetAnalogInChannel"));
        }
        Ok(())
    }

    /// The converter's count range for a channel, for turning counts into volts.
    pub fn count_range(&self, channel: u16) -> Result<(i32, i32)> {
        check_channel(channel)?;
        let mut minimum: i32 = 0;
        let mut maximum: i32 = 0;
        // SAFETY: both pointers are to live locals that outlast the call.
        let result = unsafe {
            self.api
                .HRDLGetMinMaxAdcCounts(self.handle, &mut minimum, &mut maximum, channel as i16)
        };
        if result == FAILED {
            return Err(self.failure("HRDLGetMinMaxAdcCounts"));
        }
        Ok((minimum, maximum))
    }

    /// Take one reading, waiting for the conversion.
    ///
    /// Blocks for at least the conversion time, and the header notes it blocks
    /// other driver calls too, so a unit should be read from one thread only.
    /// That suits how lumberdaq runs a device: one thread, exclusively.
    pub fn read_single(
        &self,
        channel: u16,
        range: VoltageRange,
        conversion: ConversionTime,
        single_ended: bool,
    ) -> Result<Reading> {
        check_channel(channel)?;
        let mut overflow: i16 = 0;
        let mut value: i32 = 0;
        // SAFETY: both pointers are to live locals that outlast the call.
        let result = unsafe {
            self.api.HRDLGetSingleValue(
                self.handle,
                channel as i16,
                range.as_raw(),
                conversion.as_raw(),
                i16::from(single_ended),
                &mut overflow,
                &mut value,
            )
        };
        if result == FAILED {
            return Err(self.failure("HRDLGetSingleValue"));
        }
        Ok(Reading { counts: value, overflow: overflow != 0 })
    }

    /// Ask the driver what went wrong with the last call.
    fn failure(&self, operation: &'static str) -> Error {
        Error::Failed {
            operation: operation,
            driver: DriverError::from_code(self.info_code(Info::Error)),
            settings: SettingsError::from_code(self.info_code(Info::Settings)),
        }
    }

    /// The driver returns these codes as decimal text through GetUnitInfo.
    fn info_code(&self, line: Info) -> i32 {
        match self.info_text(line) {
            Some(text) => text.trim().parse().unwrap_or(-1),
            None => -1,
        }
    }

    fn info_text(&self, line: Info) -> Option<String> {
        let mut buffer = vec![0i8; 256];
        // SAFETY: the driver writes at most stringLength bytes into buffer,
        // which is that long and outlives the call.
        let written = unsafe {
            self.api.HRDLGetUnitInfo(
                self.handle,
                buffer.as_mut_ptr(),
                buffer.len() as i16,
                line.as_raw(),
            )
        };
        if written <= 0 {
            return None;
        }
        let bytes: Vec<u8> = buffer[..written as usize]
            .iter()
            .map(|byte| *byte as u8)
            .collect();
        Some(
            String::from_utf8_lossy(&bytes)
                .trim_end_matches('\0')
                .to_string(),
        )
    }
}

impl Drop for Hrdl {
    fn drop(&mut self) {
        // SAFETY: the handle came from HRDLOpenUnit and is closed once, here.
        unsafe {
            self.api.HRDLCloseUnit(self.handle);
        }
    }
}

/// Which line of the unit description to ask for.
#[derive(Debug, Clone, Copy)]
pub enum Info {
    DriverVersion,
    UsbVersion,
    HardwareVersion,
    Variant,
    BatchAndSerial,
    CalibrationDate,
    KernelDriverVersion,
    Error,
    Settings,
}

impl Info {
    fn as_raw(self) -> i16 {
        match self {
            Info::DriverVersion => 0,
            Info::UsbVersion => 1,
            Info::HardwareVersion => 2,
            Info::Variant => 3,
            Info::BatchAndSerial => 4,
            Info::CalibrationDate => 5,
            Info::KernelDriverVersion => 6,
            Info::Error => 7,
            Info::Settings => 8,
        }
    }
}

fn check_channel(channel: u16) -> Result<()> {
    if !(MIN_CHANNEL..=MAX_CHANNEL).contains(&channel) {
        return Err(Error::ChannelOutOfRange(channel));
    }
    Ok(())
}

/// Turn converter counts into volts.
///
/// `max_counts` comes from `count_range` for the same channel, since it varies
/// with the conversion time.
pub fn counts_to_volts(counts: i32, max_counts: i32, range: VoltageRange) -> f64 {
    if max_counts == 0 {
        return 0.0;
    }
    (f64::from(counts) / f64::from(max_counts)) * (range.millivolts() / 1000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The header names the smaller ranges as rounded numbers, but each is half
    /// the one above, so the true scales are not quite what the names say.
    #[test]
    fn ranges_halve_from_full_scale() {
        assert_eq!(VoltageRange::MilliVolts2500.millivolts(), 2500.0);
        assert_eq!(VoltageRange::MilliVolts1250.millivolts(), 1250.0);
        assert_eq!(VoltageRange::MilliVolts625.millivolts(), 625.0);
        assert_eq!(VoltageRange::MilliVolts39.millivolts(), 2500.0 / 64.0);
    }

    #[test]
    fn counts_scale_against_the_range() {
        assert_eq!(counts_to_volts(0, 1_000_000, VoltageRange::MilliVolts2500), 0.0);
        assert_eq!(
            counts_to_volts(1_000_000, 1_000_000, VoltageRange::MilliVolts2500),
            2.5
        );
        assert_eq!(
            counts_to_volts(500_000, 1_000_000, VoltageRange::MilliVolts1250),
            0.625
        );
    }

    #[test]
    fn a_zero_count_range_does_not_divide_by_zero() {
        assert_eq!(counts_to_volts(123, 0, VoltageRange::MilliVolts2500), 0.0);
    }

    #[test]
    fn channels_outside_the_unit_are_rejected() {
        assert!(check_channel(0).is_err());
        assert!(check_channel(17).is_err());
        assert!(check_channel(1).is_ok());
        assert!(check_channel(16).is_ok());
    }

    /// Every setting maps to the number the header gives it.
    #[test]
    fn settings_match_the_header() {
        assert_eq!(VoltageRange::MilliVolts2500.as_raw(), 0);
        assert_eq!(VoltageRange::MilliVolts39.as_raw(), 6);
        assert_eq!(ConversionTime::Ms60.as_raw(), 0);
        assert_eq!(ConversionTime::Ms660.as_raw(), 4);
        assert_eq!(Info::Error.as_raw(), 7);
        assert_eq!(Info::Settings.as_raw(), 8);
    }

    /// Opening on a machine with no driver must say so rather than panic.
    #[test]
    fn a_missing_driver_names_what_it_looked_for() {
        if let Err(Error::DriverNotFound { tried }) = Hrdl::open() {
            assert!(!tried.is_empty());
            assert!(tried.iter().any(|path| path.contains("hrdl")));
        }
        // If a driver is present this says nothing, which is correct: the test
        // is about the failure path being reachable and readable.
    }
}
