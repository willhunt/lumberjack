use crate::Result;
use crate::datapoint::DataPoint;
use crate::channel::{ Channel, ChannelDataAquisition };
use crate::device::{ Device, DeviceInterface };
use crate::hardware::{HardwareDataAquisition, Hardware };
use serde::{ Deserialize, Serialize };
use serialport;
use chrono;
use regex::Regex;
use std::time::Duration;


/// This device reads a stream of data from a serial port in a comma-separated format, and splits it into channels according to the config.
/// The data is in a format like the following that may mix data types and also contain unwanted characters: `#1,2.00,0,1,1,STBY,0,1,0$`
/// Data types will all have to be converted to those required for DataPoint, which is a float64. 
/// The cahnnel names must be known upfront and the index of each channel in the stream must be specified in the config. The device will read all channels together and then split them into the configured channels for storage.

/// Everything needed to describe a serial device in a config file.
#[derive(Serialize, Deserialize, Clone)]
pub struct SerialStreamConfig {
    pub description: String,
    pub port: String,
    pub baudrate: u32,
    /// A regular expression matching one complete frame, used both to find
    /// frame boundaries and to strip whatever wraps the data.
    ///
    /// If the expression has a capture group, that group is the data; if not,
    /// the whole match is. So `#([^#$]*)\$` keeps what sits between the
    /// markers, and `([^\r\n]+)\r?\n` reads a device that just sends lines.
    ///
    /// The expression must require whatever ends a frame. That is what tells
    /// us a frame has fully arrived rather than being half way through the
    /// wire. A pattern that can match without its terminator, such as `(.*)`,
    /// will happily match a partial frame and hand back truncated data.
    #[serde(default = "default_frame_pattern")]
    pub frame_pattern: String,
    pub inputs: Vec<SerialStreamInput>,
}

/// Matches the `#...$` framing described above. Used when a config does not
/// name a pattern, so configs written before this setting existed still load.
fn default_frame_pattern() -> String {
    r"#([^#$]*)\$".to_string()
}

const FIELD_SEPARATOR: char = ',';

/// If this much arrives with no frame terminator in it, something is wrong with
/// the stream and we are just accumulating noise. Better to drop it than to
/// grow without bound for the rest of the run.
const MAX_BUFFER_BYTES: usize = 64 * 1024;

/// The running device: its settings, the open port once connected, and
/// whatever bytes have arrived but not yet formed a complete frame.
///
/// The port handle is the reason this type cannot derive Serialize or Clone,
/// and the reason the settings live in a separate struct that can.
pub struct SerialStream {
    config: SerialStreamConfig,
    /// The compiled form of `config.frame_pattern`. Compiling is not cheap, so
    /// it happens once here rather than on every read.
    frame_pattern: Regex,
    serial_port: Option<Box<dyn serialport::SerialPort + Send>>,
    /// Carried between reads: a frame can arrive split across two reads.
    buffer: String,
}

impl SerialStream {
    pub fn new(port: String, baudrate: u32) -> Result<SerialStream> {
        SerialStream::from_config(SerialStreamConfig {
            description: "Device streaming over serial.".to_string(),
            port: port,
            baudrate: baudrate,
            frame_pattern: default_frame_pattern(),
            inputs: vec![],
        })
    }

    /// Compiling the pattern here means a config with a bad expression is
    /// rejected when the setup is built, rather than on the first read.
    pub fn from_config(config: SerialStreamConfig) -> Result<SerialStream> {
        let frame_pattern = Regex::new(&config.frame_pattern).map_err(|error| {
            format!(
                "Frame pattern '{}' for serial port {} is not a valid regular expression: {}",
                config.frame_pattern, config.port, error
            )
        })?;
        Ok(SerialStream {
            config: config,
            frame_pattern: frame_pattern,
            serial_port: None,
            buffer: String::new(),
        })
    }

    pub fn config(&self) -> SerialStreamConfig {
        self.config.clone()
    }

    pub fn add_input(&mut self, input: SerialStreamInput) {
        self.config.inputs.push(input);
    }
}

impl DeviceInterface for SerialStream {
    fn connect(&mut self) -> Result<()> {
        let port = serialport::new(&self.config.port, self.config.baudrate)
            .timeout(Duration::from_millis(100))
            .open()?;
        self.serial_port = Some(port);
        Ok(())
    }
}


/// Pull the most recent complete frame out of the buffer, discarding any
/// earlier ones and any leading noise, and leaving a trailing partial frame in
/// place for the next read.
///
/// Returning only the latest frame is what implements "if the sample rate is
/// slower than the stream rate, only take the latest data": frames that arrived
/// while we were not looking are dropped rather than queued.
fn take_latest_frame(buffer: &mut String, pattern: &Regex) -> Option<String> {
    // Work out what to keep before touching the buffer, so the borrow the regex
    // holds on it has ended by the time we drain.
    let (consumed_to, frame) = {
        let captures = pattern.captures_iter(buffer.as_str()).last()?;
        let whole = captures.get(0)?;
        // Group 1 is the data if the pattern names one, otherwise the whole
        // match is, which lets simple patterns skip the parentheses.
        let frame = captures.get(1).unwrap_or(whole).as_str().to_string();
        (whole.end(), frame)
    };
    // Everything up to the end of that match is dealt with. Anything after it
    // is the start of a frame still arriving, so it stays for the next read.
    buffer.drain(..consumed_to);
    Some(frame)
}

/// Split one frame into a reading per configured channel.
///
/// The returned Vec is in the same order as `inputs`, because `Device::read`
/// zips it against its channels positionally.
fn parse_frame(
    frame: &str,
    inputs: &[SerialStreamInput],
    timestamp: chrono::DateTime<chrono::Utc>,
) -> Result<Vec<Vec<DataPoint>>> {
    let fields: Vec<&str> = frame.split(FIELD_SEPARATOR).map(|field| field.trim()).collect();
    let mut readings: Vec<Vec<DataPoint>> = Vec::with_capacity(inputs.len());

    for input in inputs.iter() {
        match input {
            SerialStreamInput::LineInput { index } => {
                let position = usize::try_from(*index).map_err(|_| {
                    format!("Channel index {} is negative.", index)
                })?;
                let field = fields.get(position).ok_or_else(|| {
                    format!(
                        "A channel is configured to read index {}, but the frame has only {} fields: '{}'",
                        index, fields.len(), frame
                    )
                })?;
                let value: f64 = field.parse().map_err(|_| {
                    format!(
                        "Could not read a number from index {} of frame '{}'. The field was '{}'.",
                        index, frame, field
                    )
                })?;
                readings.push(vec![DataPoint { datetime: timestamp, value: value }]);
            }
        }
    }
    Ok(readings)
}

impl HardwareDataAquisition for SerialStream {
    /// Read from device.
    /// If the sampling rate is higher than the stream rate there may not be any data.
    /// If the sample rate is slower than the stream rate, only take the latest data.
    fn read(&mut self) -> Result<Vec<Vec<DataPoint>>> {
        let port = match &mut self.serial_port {
            Some(port) => port,
            None => return Err("Serial device is not connected.".into()),
        };

        // Take only what has already arrived. Asking first means we never block
        // for the port timeout when the device simply has not sent anything.
        let available = port.bytes_to_read()? as usize;
        if available > 0 {
            let mut bytes = vec![0u8; available];
            let read_count = port.read(&mut bytes)?;
            // The frame format is ASCII, so a lossy conversion is safe here.
            self.buffer.push_str(&String::from_utf8_lossy(&bytes[..read_count]));
        }

        if self.buffer.len() > MAX_BUFFER_BYTES {
            self.buffer.clear();
            return Err("Serial buffer filled with no complete frame; discarding it.".into());
        }

        match take_latest_frame(&mut self.buffer, &self.frame_pattern) {
            // Nothing complete yet: we are sampling faster than the device sends.
            None => Ok(vec![]),
            Some(frame) => parse_frame(&frame, &self.config.inputs, chrono::Utc::now()),
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub enum SerialStreamInput {
    LineInput { index: i64 },
}
impl ChannelDataAquisition for SerialStreamInput {
    fn read(&mut self) -> Result<Vec<DataPoint>> {
        match self {
            SerialStreamInput::LineInput {index: _} => {
                Err("Channels for this device must be read all together by the device read method.".into())
            },
        }
    }
}

pub fn create_device(name: String, description: String, port: String, baudrate: u32) -> Result<Device> {
    let hardware = SerialStream::new(port, baudrate)?;
    Ok(Device::new(name, description, Hardware::SerialStream(hardware)))
}

pub fn add_channel(device: &mut Device, name: String, description: String, index: i64, unit: String) -> Result<()> {
    match &mut device.hardware {
        Hardware::SerialStream(hardware) => {
            hardware.add_input(SerialStreamInput::LineInput { index: index });
        },
        _ => {
            return Err("This channel can only be added to a serial stream device.".into())
        }
    }

    let channel = Channel::new(
        index.to_string(),
        name,
        unit,
        description,
    );
    device.add_channel(channel)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The example frame from the device documentation above.
    const EXAMPLE: &str = "1,2.00,0,1,1,STBY,0,1,0";

    fn line_inputs(indices: &[i64]) -> Vec<SerialStreamInput> {
        indices.iter().map(|index| SerialStreamInput::LineInput { index: *index }).collect()
    }

    #[test]
    fn reads_the_configured_indices_in_order() {
        let readings = parse_frame(EXAMPLE, &line_inputs(&[1, 3]), chrono::Utc::now()).unwrap();
        assert_eq!(readings.len(), 2);
        assert_eq!(readings[0][0].value, 2.00);
        assert_eq!(readings[1][0].value, 1.0);
    }

    #[test]
    fn every_channel_in_a_frame_shares_one_timestamp() {
        let readings = parse_frame(EXAMPLE, &line_inputs(&[0, 1, 2]), chrono::Utc::now()).unwrap();
        assert_eq!(readings[0][0].datetime, readings[1][0].datetime);
        assert_eq!(readings[1][0].datetime, readings[2][0].datetime);
    }

    #[test]
    fn a_non_numeric_field_is_rejected() {
        // Index 5 is "STBY". Leaving it out of the config is how you skip it.
        let result = parse_frame(EXAMPLE, &line_inputs(&[5]), chrono::Utc::now());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("STBY"));
    }

    #[test]
    fn an_index_past_the_end_of_the_frame_is_rejected() {
        let result = parse_frame(EXAMPLE, &line_inputs(&[99]), chrono::Utc::now());
        assert!(result.is_err());
    }

    fn default_pattern() -> Regex {
        Regex::new(&default_frame_pattern()).unwrap()
    }

    #[test]
    fn takes_the_latest_frame_and_keeps_the_partial_one() {
        let mut buffer = String::from("#1,2.00$#1,3.00$#1,4.0");
        assert_eq!(take_latest_frame(&mut buffer, &default_pattern()).unwrap(), "1,3.00");
        assert_eq!(buffer, "#1,4.0");
    }

    #[test]
    fn an_incomplete_frame_yields_nothing_and_is_kept() {
        let mut buffer = String::from("#1,2.0");
        assert!(take_latest_frame(&mut buffer, &default_pattern()).is_none());
        assert_eq!(buffer, "#1,2.0");
    }

    #[test]
    fn a_frame_split_across_two_reads_is_rejoined() {
        let mut buffer = String::from("#1,2.");
        assert!(take_latest_frame(&mut buffer, &default_pattern()).is_none());
        buffer.push_str("00,3$");
        assert_eq!(take_latest_frame(&mut buffer, &default_pattern()).unwrap(), "1,2.00,3");
    }

    #[test]
    fn noise_before_a_frame_is_discarded() {
        let mut buffer = String::from("garbage#1,2.00$");
        assert_eq!(take_latest_frame(&mut buffer, &default_pattern()).unwrap(), "1,2.00");
    }

    /// A device with no framing characters at all, just newline terminated
    /// lines. This is the common case the default pattern does not cover.
    #[test]
    fn a_newline_terminated_device_needs_only_a_different_pattern() {
        let pattern = Regex::new(r"([^\r\n]+)\r?\n").unwrap();
        let mut buffer = String::from("1,2.00\r\n1,3.00\r\n1,4.0");
        assert_eq!(take_latest_frame(&mut buffer, &pattern).unwrap(), "1,3.00");
        assert_eq!(buffer, "1,4.0");
    }

    /// Without a capture group the whole match is the data, so a pattern that
    /// needs no cleaning can skip the parentheses.
    #[test]
    fn a_pattern_without_a_capture_group_uses_the_whole_match() {
        let pattern = Regex::new(r"[0-9.,]+;").unwrap();
        let mut buffer = String::from("1,2.00;1,3.00;");
        assert_eq!(take_latest_frame(&mut buffer, &pattern).unwrap(), "1,3.00;");
    }

    /// Devices that wrap data in something more than one character, here a
    /// checksum that should not reach the parser.
    #[test]
    fn a_pattern_can_strip_more_than_delimiters() {
        let pattern = Regex::new(r"\$DATA,([^*]*)\*[0-9A-F]{2}\r\n").unwrap();
        let mut buffer = String::from("$DATA,1,2.00,3*7F\r\n");
        assert_eq!(take_latest_frame(&mut buffer, &pattern).unwrap(), "1,2.00,3");
    }

    /// Configs written before frame_pattern existed must still load.
    #[test]
    fn a_config_without_a_pattern_falls_back_to_the_default() {
        let json = r#"{
            "description": "Older config",
            "port": "COM3",
            "baudrate": 115200,
            "inputs": []
        }"#;
        let config: SerialStreamConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.frame_pattern, default_frame_pattern());
    }

    #[test]
    fn a_bad_pattern_is_rejected_when_the_device_is_built() {
        let config = SerialStreamConfig {
            description: "Broken".to_string(),
            port: "COM1".to_string(),
            baudrate: 9600,
            frame_pattern: r"#([$".to_string(),
            inputs: vec![],
        };
        let error = SerialStream::from_config(config).err().unwrap();
        assert!(error.to_string().contains("not a valid regular expression"));
    }
}
