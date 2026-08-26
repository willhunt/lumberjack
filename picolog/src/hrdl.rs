//! ADC-20 and ADC-24 high resolution data loggers.
//!
//! A safe layer over `src/raw/hrdl.rs`. The driver is opened at runtime rather
//! than linked, so a machine with no Pico software installed still builds and
//! runs; it fails here, with something worth reading.

use crate::raw::hrdl::PicoHrdl;
use std::fmt;
use std::time::Duration;

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

/// Whether a channel can be the primary of a differential pair.
///
/// A differential input pairs a channel with the one above it, so the primary
/// is always odd and the even channel beside it is consumed by the pair. An
/// ADC-20 therefore offers eight single ended inputs or four differential ones.
pub fn can_be_differential(channel: u16) -> bool {
    channel % 2 == 1
}

/// The channel consumed alongside `primary` when it is used differentially.
pub fn differential_partner(primary: u16) -> u16 {
    primary + 1
}

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
            // The header defines 0 to 5, but a real ADC-20 returns 8 through
            // HRDL_ERROR after a rejected channel, so this is reachable.
            DriverError::Unknown(code) => {
                return write!(formatter, "the driver reported code {}", code)
            }
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
    /// The unit reported a model this crate does not know the shape of.
    UnknownVariant { variant: String },
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
                // The settings code says which argument was objected to and is
                // the useful one. The driver code is reported only when the
                // settings are fine, which means the failure was something
                // else. Observed on a real ADC-20: a bad channel gives settings
                // 3 and driver 8, and 8 is not a value the header defines, so
                // printing it alongside a perfectly good explanation was only
                // ever noise.
                match settings {
                    SettingsError::Ok => write!(formatter, "{} failed: {}", operation, driver),
                    _ => write!(formatter, "{} failed: {}", operation, settings),
                }
            }
            Error::ChannelOutOfRange(channel) => write!(
                formatter,
                "channel {} is outside the {}..={} the unit provides",
                channel, MIN_CHANNEL, MAX_CHANNEL
            ),
            Error::UnknownVariant { variant } => write!(
                formatter,
                "the unit reports variant {}, which is neither an ADC-20 nor an ADC-24",
                variant
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

/// One complete sweep of the enabled channels, taken by the unit itself.
#[derive(Debug, Clone)]
pub struct Scan {
    /// When the unit took this scan, measured from the start of streaming.
    ///
    /// The unit's own clock rather than ours, so it does not inherit any
    /// lateness in when we got round to draining the buffer.
    pub since_start: Duration,
    /// Raw counts, one per enabled channel, in the order they were enabled.
    pub counts: Vec<i32>,
    /// An input went outside its range during this drain.
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
            HRDLSetInterval,
            HRDLRun,
            HRDLReady,
            HRDLStop,
            HRDLGetTimesAndValues,
        );

        // SAFETY: takes no arguments and returns a handle by value.
        let handle = unsafe { api.HRDLOpenUnit() };
        if handle <= 0 {
            return Err(Error::NoUnitFound);
        }
        Ok(Hrdl { api: api, handle: handle })
    }

    /// How many analog inputs this unit has.
    ///
    /// Read from the variant rather than assumed: an ADC-20 has eight and an
    /// ADC-24 sixteen, and asking is the only way to tell them apart.
    pub fn channel_count(&self) -> Result<u16> {
        let variant = self.info(Info::Variant)?;
        match variant.trim() {
            "20" => Ok(8),
            "24" => Ok(16),
            other => Err(Error::UnknownVariant { variant: other.to_string() }),
        }
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


    // -- Streaming ----------------------------------------------------------
    //
    // The alternative to asking for one value at a time. The unit scans on its
    // own schedule into a driver side buffer, and we drain it. Two things come
    // out of that which single shot cannot give: every sample carries the time
    // the *unit* took it, and there is no per call cost for switching input,
    // because the unit is sweeping the channels itself.

    /// How often the unit should take a complete scan, and how long each
    /// channel converts for.
    ///
    /// The driver rejects an interval too short for the conversion time and
    /// channel count, which surfaces here as `ConversionTimeTooSlow`.
    pub fn set_interval(&mut self, interval: Duration, conversion: ConversionTime) -> Result<()> {
        // SAFETY: three integers by value.
        let result = unsafe {
            self.api.HRDLSetInterval(
                self.handle,
                interval.as_millis() as i32,
                conversion.as_raw(),
            )
        };
        if result == FAILED {
            return Err(self.failure("HRDLSetInterval"));
        }
        Ok(())
    }

    /// Begin streaming into a buffer of `buffer_scans` complete scans.
    ///
    /// The buffer is what bounds how long a reader can be away before samples
    /// are lost, so it should hold comfortably more than one drain's worth.
    pub fn start_streaming(&mut self, buffer_scans: u32) -> Result<()> {
        // SAFETY: three integers by value. HRDL_BM_STREAM is 2.
        let result = unsafe { self.api.HRDLRun(self.handle, buffer_scans as i32, 2) };
        if result == FAILED {
            return Err(self.failure("HRDLRun"));
        }
        Ok(())
    }

    /// Whether the unit has finished starting up and has samples to give.
    pub fn ready(&self) -> bool {
        // SAFETY: one integer by value.
        unsafe { self.api.HRDLReady(self.handle) != 0 }
    }

    /// Take whatever the unit has collected since the last call.
    ///
    /// Returns at most `max_scans`. An empty result is normal rather than an
    /// error: it means nothing new has been converted yet.
    ///
    /// `channel_count` must be the number of channels actually enabled. The
    /// driver interleaves the buffer by scan, so getting this wrong would
    /// silently shear the values across channels.
    pub fn take_scans(&self, channel_count: usize, max_scans: usize) -> Result<Vec<Scan>> {
        if channel_count == 0 || max_scans == 0 {
            return Ok(Vec::new());
        }
        let mut times = vec![0i32; max_scans];
        let mut values = vec![0i32; max_scans * channel_count];
        let mut overflow: i16 = 0;

        // SAFETY: both buffers are sized for max_scans as promised by the last
        // argument, and outlive the call.
        let collected = unsafe {
            self.api.HRDLGetTimesAndValues(
                self.handle,
                times.as_mut_ptr(),
                values.as_mut_ptr(),
                &mut overflow,
                max_scans as i32,
            )
        };
        if collected < 0 {
            return Err(self.failure("HRDLGetTimesAndValues"));
        }

        // The driver reports one overflow flag for the whole drain rather than
        // per scan, so it applies to all of them or none.
        let overflowed = overflow != 0;
        let mut scans = Vec::with_capacity(collected as usize);
        for index in 0..collected as usize {
            let start = index * channel_count;
            scans.push(Scan {
                since_start: Duration::from_millis(times[index].max(0) as u64),
                counts: values[start..start + channel_count].to_vec(),
                overflow: overflowed,
            });
        }
        Ok(scans)
    }

    /// Stop streaming. Safe to call whether or not it was started.
    pub fn stop(&mut self) {
        // SAFETY: one integer by value, and this one returns nothing.
        unsafe { self.api.HRDLStop(self.handle) }
    }

    /// Ask the driver what went wrong with the last call.
    ///
    /// These must be read before anything else is asked of the unit. A
    /// successful `HRDLGetUnitInfo` resets the settings code to SE_OK, so
    /// reading the other lines first would report that nothing was wrong.
    /// Confirmed on a real ADC-20: a failure that reported settings code 3
    /// read back as 9 once seven other info lines had been fetched.
    fn failure(&self, operation: &'static str) -> Error {
        let settings = SettingsError::from_code(self.info_code(Info::Settings));
        let driver = DriverError::from_code(self.info_code(Info::Error));
        Error::Failed { operation: operation, driver: driver, settings: settings }
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
        // Stopping first so the unit is not left converting into a buffer
        // nobody will read.
        unsafe {
            self.api.HRDLStop(self.handle);
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

    /// Pico's own example: "Primary inputs for differential pairs are odd
    /// channel numbers eg 1, 3, 5, etc. Their corresponding secondary numbers
    /// are primary channel number + 1".
    /// The settings code carries the useful explanation, so it is what a
    /// message should lead with. Reporting the driver code alongside it was
    /// noise, and the value observed there is not one the header defines.
    #[test]
    fn a_failure_reports_the_settings_complaint_not_the_driver_code() {
        let failure = Error::Failed {
            operation: "HRDLSetAnalogInChannel",
            driver: DriverError::Unknown(8),
            settings: SettingsError::ChannelNotAvailable,
        };
        let message = failure.to_string();
        assert!(message.contains("channel not available"));
        assert!(!message.contains("8"), "driver code should not be reported: {}", message);
    }

    /// When the settings were accepted the failure was something else, so then
    /// the driver code is all there is to go on.
    #[test]
    fn a_failure_with_good_settings_falls_back_to_the_driver_code() {
        let failure = Error::Failed {
            operation: "HRDLOpenUnit",
            driver: DriverError::NotFound,
            settings: SettingsError::Ok,
        };
        assert!(failure.to_string().contains("no unit found"));
    }

    #[test]
    fn only_odd_channels_can_lead_a_differential_pair() {
        assert!(can_be_differential(1));
        assert!(can_be_differential(7));
        assert!(!can_be_differential(2));
        assert!(!can_be_differential(8));
        assert_eq!(differential_partner(1), 2);
        assert_eq!(differential_partner(7), 8);
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
