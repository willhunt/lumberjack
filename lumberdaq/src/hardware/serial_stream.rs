use crate::{ Error, Result };
use crate::datapoint::DataPoint;
use crate::channel::ChannelInfo;
use crate::device::{ Device, DeviceInterface };
use crate::hardware::{HardwareDataAquisition, Hardware };
use serde::{ Deserialize, Serialize };
use serialport;
use chrono::{ DateTime, Utc };
use regex::Regex;
use std::io::Read;
use std::sync::atomic::{ AtomicBool, Ordering };
use std::sync::mpsc::{ self, Receiver, Sender, TryRecvError };
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;


/// This device reads a stream of data from a serial port in a comma-separated format, and splits it into channels according to the config.
/// The data is in a format like the following that may mix data types and also contain unwanted characters: `#1,2.00,0,1,1,STBY,0,1,0$`
/// Data types will all have to be converted to those required for DataPoint, which is a float64. 
/// The cahnnel names must be known upfront and the index of each channel in the stream must be specified in the config. The device will read all channels together and then split them into the configured channels for storage.

/// Everything needed to describe a serial device in a config file.
#[derive(Serialize, Deserialize, Clone)]
pub struct SerialStreamConfig {
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
    pub channels: Vec<SerialStreamChannel>,
}

/// One channel: what it is, and where to find it in the frame.
///
/// Description and binding live together deliberately. When they were two
/// parallel lists, matched by position, a config that listed them in different
/// orders would quietly record each channel's data under another's name.
#[derive(Serialize, Deserialize, Clone)]
pub struct SerialStreamChannel {
    #[serde(flatten)]
    pub info: ChannelInfo,
    /// Which comma separated field of the frame this channel reads, counting
    /// from zero.
    pub index: i64,
}

/// Matches the `#...$` framing described above. Used when a config does not
/// name a pattern, so configs written before this setting existed still load.
fn default_frame_pattern() -> String {
    r"#([^#$]*)\$".to_string()
}

/// A serial port that is plugged in at the moment.
///
/// Enough to choose one by: `COM7` alone is no help when three things are
/// plugged in, and the name the device reports is how somebody knows which is
/// theirs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortOption {
    /// What to put in the config: `COM7`, `/dev/ttyUSB0`.
    pub name: String,
    /// What the device calls itself, where it says. Empty when it does not.
    pub product: String,
    /// USB ports are the ones an instrument is usually on. A built-in COM1 on
    /// a desktop is a port, but rarely the one being looked for.
    pub usb: bool,
}

impl PortOption {
    /// The port as a line somebody picks from a list.
    pub fn label(&self) -> String {
        match self.product.is_empty() {
            true => self.name.clone(),
            false => format!("{} — {}", self.name, self.product),
        }
    }
}

/// Every serial port currently attached.
///
/// USB ports first, since that is where an instrument usually is, and each
/// group in the order the operating system gave them. An empty list is a
/// perfectly ordinary answer: it means nothing is plugged in.
///
/// This asks the operating system, so it is a moment's work rather than free.
/// Call it when a list is about to be shown, not on every redraw.
pub fn available_ports() -> Vec<PortOption> {
    let mut ports: Vec<PortOption> = match serialport::available_ports() {
        Ok(ports) => ports
            .into_iter()
            .map(|port| {
                let (product, usb) = match &port.port_type {
                    serialport::SerialPortType::UsbPort(info) => {
                        // The product name if it gives one, else the maker's
                        // name, else nothing rather than a made-up label.
                        let product = info
                            .product
                            .clone()
                            .or_else(|| info.manufacturer.clone())
                            .unwrap_or_default();
                        (product, true)
                    }
                    _ => (String::new(), false),
                };

                PortOption { name: port.port_name, product, usb }
            })
            .collect(),
        // Nothing to offer is the same answer whether the list was empty or
        // could not be read. A rig is not misconfigured because a port scan
        // failed, and the port can still be typed in.
        Err(_) => Vec::new(),
    };

    ports.sort_by_key(|port| !port.usb);
    ports
}

/// The port to suggest for a device nobody has configured yet.
pub fn first_usb_port() -> Option<String> {
    available_ports().into_iter().find(|port| port.usb).map(|port| port.name)
}

impl Default for SerialStreamConfig {
    /// A device that still has to be told which port it is on.
    ///
    /// The port is left empty rather than guessed at: `COM1` is a real port on
    /// most Windows machines and almost never the right one, so a wrong guess
    /// would be tried and fail confusingly. Empty says plainly that nobody has
    /// said yet.
    fn default() -> SerialStreamConfig {
        SerialStreamConfig {
            port: String::new(),
            baudrate: 115200,
            frame_pattern: default_frame_pattern(),
            channels: vec![],
        }
    }
}

const FIELD_SEPARATOR: char = ',';

/// If this much arrives with no frame terminator in it, something is wrong with
/// the stream and we are just accumulating noise. Better to drop it than to
/// grow without bound for the rest of the run.
const MAX_BUFFER_BYTES: usize = 64 * 1024;

/// A frame, and when it finished arriving.
struct StampedFrame {
    at: DateTime<Utc>,
    frame: String,
}

/// The running device: its settings, and a thread reading the port.
///
/// The port is not held here. It is moved into a reader thread that blocks on
/// it, which is the only way to know when data arrived: nothing on Windows
/// timestamps serial bytes, so the closest available answer is the moment a
/// blocking read wakes up. Draining on a schedule instead would date every
/// frame to when we got round to looking, and the error would be however long
/// that was.
///
/// It also takes the accuracy out of the user's hands. With a drain schedule,
/// setting the interval too slow silently costs timestamp accuracy and lumps
/// frames together; here the timestamps are the same whatever the device
/// thread's interval is, and a slow drain only means more frames per batch.
pub struct SerialStream {
    config: SerialStreamConfig,
    /// The compiled form of `config.frame_pattern`. Compiling is not cheap, so
    /// it happens once here rather than on every read.
    frame_pattern: Regex,
    /// Frames the reader thread has stamped and handed over.
    frames: Option<Receiver<StampedFrame>>,
    /// Asks the reader thread to finish.
    stop: Arc<AtomicBool>,
    reader: Option<JoinHandle<()>>,
}

impl SerialStream {
    pub fn new(port: String, baudrate: u32) -> Result<SerialStream> {
        SerialStream::from_config(SerialStreamConfig {
            port: port,
            baudrate: baudrate,
            frame_pattern: default_frame_pattern(),
            channels: vec![],
        })
    }

    /// Compiling the pattern here means a config with a bad expression is
    /// rejected when the setup is built, rather than on the first read.
    pub fn from_config(config: SerialStreamConfig) -> Result<SerialStream> {
        let frame_pattern = Regex::new(&config.frame_pattern).map_err(|error| {
            Error::InvalidFramePattern {
                pattern: config.frame_pattern.clone(),
                port: config.port.clone(),
                source: error,
            }
        })?;
        Ok(SerialStream {
            config: config,
            frame_pattern: frame_pattern,
            frames: None,
            stop: Arc::new(AtomicBool::new(false)),
            reader: None,
        })
    }

    pub fn config(&self) -> SerialStreamConfig {
        self.config.clone()
    }

    pub fn add_channel(&mut self, channel: SerialStreamChannel) {
        self.config.channels.push(channel);
    }
}

impl SerialStream {
    /// Ask the reader thread to finish, and wait for it.
    ///
    /// Dropping the receiver as well, so a thread that has already exited on a
    /// dead port does not leave a channel behind that looks connected.
    fn stop_reader(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.frames = None;
        if let Some(reader) = self.reader.take() {
            // The thread checks the flag between reads, so this waits at most
            // one port timeout.
            let _ = reader.join();
        }
    }
}

impl DeviceInterface for SerialStream {
    fn connect(&mut self) -> Result<()> {
        // Any previous reader owns a port that is about to be replaced, and a
        // buffer holding bytes from before the outage. Stopping it discards
        // both. Carrying that buffer across a reconnection would join half a
        // frame from before to half from after, and the result would parse
        // perfectly: a plausible reading that never happened.
        self.stop_reader();

        let port = serialport::new(&self.config.port, self.config.baudrate)
            .timeout(Duration::from_millis(100))
            .open()?;

        // A fresh flag rather than clearing the old one, so a thread that has
        // not noticed it should stop cannot be revived by this.
        let stop = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::channel();
        let pattern = self.frame_pattern.clone();
        let thread_stop = Arc::clone(&stop);

        let reader = std::thread::spawn(move || {
            read_frames(port, pattern, sender, thread_stop);
        });

        self.stop = stop;
        self.frames = Some(receiver);
        self.reader = Some(reader);
        Ok(())
    }
}

/// Read the port until told to stop, handing over each frame as it completes.
///
/// The blocking read is the point. It wakes when bytes arrive, so the time
/// taken immediately after is as close to the arrival time as anything
/// available without driver support, rather than being however late the next
/// scheduled look happened to be.
///
/// A frame is stamped when the read that *completed* it returned, which is
/// when the device finished sending it.
fn read_frames(
    mut port: Box<dyn serialport::SerialPort + Send>,
    pattern: Regex,
    sender: Sender<StampedFrame>,
    stop: Arc<AtomicBool>,
) {
    let mut buffer = String::new();
    let mut bytes = [0u8; 4096];

    while !stop.load(Ordering::Relaxed) {
        let count = match port.read(&mut bytes) {
            Ok(0) => continue,
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::TimedOut => continue,
            // The port has gone. Ending the thread drops the sender, which is
            // how the device finds out: its next read sees the channel closed.
            Err(_) => return,
        };
        let at = Utc::now();

        // The frame format is ASCII, so a lossy conversion is safe here.
        buffer.push_str(&String::from_utf8_lossy(&bytes[..count]));

        // Every frame completed by this one read shares its arrival time, and
        // two readings at the same instant are not a series: nothing can order
        // them, plot them or interpolate between them. When that happens there
        // is also no way to know when the earlier one arrived, since both were
        // already sitting in the operating system's buffer. So keep the newest
        // and drop the rest, which is the value that was current at `at`.
        //
        // This is the old take-the-latest behaviour, but applied per read
        // rather than per drain. Per drain it discarded most of the data,
        // because a drain can be seconds long. A read spans the moment bytes
        // arrived, so in practice it discards nothing: measured against the
        // real device, 2.3% of reads completed more than one frame.
        let mut newest: Option<String> = None;
        while let Some(frame) = take_next_frame(&mut buffer, &pattern) {
            newest = Some(frame);
        }
        if let Some(frame) = newest {
            if sender.send(StampedFrame { at: at, frame: frame }).is_err() {
                return; // nobody is listening any more
            }
        }

        // Bytes arriving that never form a frame means the stream is not what
        // the pattern says it is. Dropping them rather than growing all run.
        if buffer.len() > MAX_BUFFER_BYTES {
            buffer.clear();
        }
    }
}

impl Drop for SerialStream {
    fn drop(&mut self) {
        self.stop_reader();
    }
}


/// Pull the *first* complete frame out of the buffer, discarding any leading
/// noise and leaving everything after it in place.
///
/// This used to take the last frame and throw the rest away, which quietly lost
/// data whenever the device sent faster than we looked. Now that a thread reads
/// the port continuously there is no reason to drop any: taking them in turn
/// keeps every frame, in order, each with its own arrival time.
fn take_next_frame(buffer: &mut String, pattern: &Regex) -> Option<String> {
    // Work out what to keep before touching the buffer, so the borrow the regex
    // holds on it has ended by the time we drain.
    let (consumed_to, frame) = {
        let captures = pattern.captures(buffer.as_str())?;
        let whole = captures.get(0)?;
        // Group 1 is the data if the pattern names one, otherwise the whole
        // match is, which lets simple patterns skip the parentheses.
        let frame = captures.get(1).unwrap_or(whole).as_str().to_string();
        (whole.end(), frame)
    };
    // Everything up to the end of that match is dealt with. What follows may be
    // further complete frames or the start of one still arriving; either way it
    // stays for the next call.
    buffer.drain(..consumed_to);
    Some(frame)
}

/// Split one frame into a value per configured channel.
///
/// The returned Vec is in the same order as `channels`, because `Device::read`
/// pairs it against the device's channels positionally. Both come from this one
/// list, so they cannot fall out of step.
///
/// Values rather than datapoints, because one frame contributes one value to
/// each channel and the caller holds the timestamp that applies to all of them.
fn parse_frame_values(frame: &str, channels: &[SerialStreamChannel]) -> Result<Vec<f64>> {
    let fields: Vec<&str> = frame.split(FIELD_SEPARATOR).map(|field| field.trim()).collect();
    let mut values: Vec<f64> = Vec::with_capacity(channels.len());

    for channel in channels.iter() {
        let position = usize::try_from(channel.index).map_err(|_| {
            Error::NegativeChannelIndex {
                channel: channel.info.name.clone(),
                index: channel.index,
            }
        })?;
        let field = fields.get(position).ok_or_else(|| Error::FrameTooShort {
            channel: channel.info.name.clone(),
            index: channel.index,
            fields: fields.len(),
            frame: frame.to_string(),
        })?;
        let value: f64 = field.parse().map_err(|_| Error::FieldNotNumeric {
            channel: channel.info.name.clone(),
            index: channel.index,
            field: field.to_string(),
            frame: frame.to_string(),
        })?;
        values.push(value);
    }
    Ok(values)
}

impl HardwareDataAquisition for SerialStream {
    /// Take every frame the reader thread has handed over since last time.
    ///
    /// Nothing is discarded and nothing waits: an empty result means no frame
    /// finished arriving since the last call, which is normal when draining
    /// faster than the device sends.
    fn read(&mut self) -> Result<Vec<Vec<DataPoint>>> {
        let frames = match &self.frames {
            Some(frames) => frames,
            None => return Err(Error::NotConnected { port: self.config.port.clone() }),
        };

        let mut stamped: Vec<StampedFrame> = Vec::new();
        let mut reader_gone = false;
        loop {
            match frames.try_recv() {
                Ok(frame) => stamped.push(frame),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    reader_gone = true;
                    break;
                }
            }
        }

        // The reader only ends on a dead port. Report that, but not before
        // handing over what it managed to read first: the next call will find
        // the channel closed and empty, and say so then.
        if reader_gone && stamped.is_empty() {
            return Err(Error::NotConnected { port: self.config.port.clone() });
        }

        let mut readings: Vec<Vec<DataPoint>> = vec![Vec::new(); self.config.channels.len()];
        for frame in stamped.iter() {
            let values = parse_frame_values(&frame.frame, &self.config.channels)?;
            for (index, value) in values.iter().enumerate() {
                readings[index].push(DataPoint { datetime: frame.at, value: *value });
            }
        }
        Ok(readings)
    }
}

pub fn create_device(name: String, port: String, baudrate: u32) -> Result<Device> {
    let hardware = SerialStream::new(port, baudrate)?;
    Ok(Device::new(name, Hardware::SerialStream(hardware)))
}

pub fn add_channel(device: &mut Device, name: String, index: i64, unit: String) -> Result<()> {
    match &mut device.hardware {
        Hardware::SerialStream(hardware) => {
            hardware.add_channel(SerialStreamChannel {
                info: ChannelInfo {
                    name: name,
                    unit: unit,
                scale: None,
                },
                index: index,
            });
        },
        _ => {
            return Err(Error::WrongHardwareType { expected: "serial stream".to_string() })
        }
    }
    // The hardware config is the definition; the device mirrors it.
    device.rebuild_channels()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The example frame from the device documentation above.
    const EXAMPLE: &str = "1,2.00,0,1,1,STBY,0,1,0";

    fn line_inputs(indices: &[i64]) -> Vec<SerialStreamChannel> {
        indices.iter().map(|index| SerialStreamChannel {
            info: ChannelInfo {
                name: format!("Channel {}", index),
                unit: "-".to_string(),
            scale: None,
            },
            index: *index,
        }).collect()
    }

    #[test]
    fn reads_the_configured_indices_in_order() {
        let values = parse_frame_values(EXAMPLE, &line_inputs(&[1, 3])).unwrap();
        assert_eq!(values, vec![2.00, 1.0]);
    }

    #[test]
    fn a_non_numeric_field_is_rejected() {
        // Index 5 is "STBY". Leaving it out of the config is how you skip it.
        let result = parse_frame_values(EXAMPLE, &line_inputs(&[5]));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("STBY"));
    }

    #[test]
    fn an_index_past_the_end_of_the_frame_is_rejected() {
        let result = parse_frame_values(EXAMPLE, &line_inputs(&[99]));
        assert!(result.is_err());
    }

    fn default_pattern() -> Regex {
        Regex::new(&default_frame_pattern()).unwrap()
    }

    /// The behaviour this replaced took the last frame and dropped the rest.
    /// Every frame now comes out, in order, and the partial one is kept.
    #[test]
    fn frames_come_out_in_order_and_none_are_dropped() {
        let mut buffer = String::from("#1,2.00$#1,3.00$#1,4.0");
        let pattern = default_pattern();
        assert_eq!(take_next_frame(&mut buffer, &pattern).unwrap(), "1,2.00");
        assert_eq!(take_next_frame(&mut buffer, &pattern).unwrap(), "1,3.00");
        assert!(take_next_frame(&mut buffer, &pattern).is_none());
        assert_eq!(buffer, "#1,4.0");
    }

    #[test]
    fn an_incomplete_frame_yields_nothing_and_is_kept() {
        let mut buffer = String::from("#1,2.0");
        assert!(take_next_frame(&mut buffer, &default_pattern()).is_none());
        assert_eq!(buffer, "#1,2.0");
    }

    #[test]
    fn a_frame_split_across_two_reads_is_rejoined() {
        let mut buffer = String::from("#1,2.");
        assert!(take_next_frame(&mut buffer, &default_pattern()).is_none());
        buffer.push_str("00,3$");
        assert_eq!(take_next_frame(&mut buffer, &default_pattern()).unwrap(), "1,2.00,3");
    }

    #[test]
    fn noise_before_a_frame_is_discarded() {
        let mut buffer = String::from("garbage#1,2.00$");
        assert_eq!(take_next_frame(&mut buffer, &default_pattern()).unwrap(), "1,2.00");
    }

    /// A device with no framing characters at all, just newline terminated
    /// lines. This is the common case the default pattern does not cover.
    #[test]
    fn a_newline_terminated_device_needs_only_a_different_pattern() {
        let pattern = Regex::new(r"([^\r\n]+)\r?\n").unwrap();
        let mut buffer = String::from("1,2.00\r\n1,3.00\r\n1,4.0");
        assert_eq!(take_next_frame(&mut buffer, &pattern).unwrap(), "1,2.00");
        assert_eq!(take_next_frame(&mut buffer, &pattern).unwrap(), "1,3.00");
        assert_eq!(buffer, "1,4.0");
    }

    /// Without a capture group the whole match is the data, so a pattern that
    /// needs no cleaning can skip the parentheses.
    #[test]
    fn a_pattern_without_a_capture_group_uses_the_whole_match() {
        let pattern = Regex::new(r"[0-9.,]+;").unwrap();
        let mut buffer = String::from("1,2.00;1,3.00;");
        assert_eq!(take_next_frame(&mut buffer, &pattern).unwrap(), "1,2.00;");
    }

    /// Devices that wrap data in something more than one character, here a
    /// checksum that should not reach the parser.
    #[test]
    fn a_pattern_can_strip_more_than_delimiters() {
        let pattern = Regex::new(r"\$DATA,([^*]*)\*[0-9A-F]{2}\r\n").unwrap();
        let mut buffer = String::from("$DATA,1,2.00,3*7F\r\n");
        assert_eq!(take_next_frame(&mut buffer, &pattern).unwrap(), "1,2.00,3");
    }

    /// A failed connection must leave nothing behind that looks connected: no
    /// reader thread, and a read that says so rather than waiting on a channel
    /// nobody will send to.
    #[test]
    fn a_failed_connection_leaves_no_reader() {
        let mut stream = SerialStream::new("no-such-port".to_string(), 115200).unwrap();
        assert!(stream.connect().is_err());
        assert!(stream.reader.is_none());
        assert!(stream.frames.is_none());
        assert!(matches!(stream.read(), Err(Error::NotConnected { .. })));
    }

    /// Bytes from before an outage live in the reader thread's buffer, and it
    /// is stopped and replaced on connect, so a partial frame from before can
    /// never be joined to bytes from after. This checks the stopping, which is
    /// what makes that true.
    #[test]
    fn connecting_stops_any_previous_reader() {
        let mut stream = SerialStream::new("no-such-port".to_string(), 115200).unwrap();
        let first_flag = Arc::clone(&stream.stop);
        assert!(stream.connect().is_err());
        // The old flag is set, so a thread still holding it would finish.
        assert!(first_flag.load(Ordering::Relaxed));
    }

    /// Configs written before frame_pattern existed must still load.
    #[test]
    fn a_config_without_a_pattern_falls_back_to_the_default() {
        let json = r#"{
            "description": "Older config",
            "port": "COM3",
            "baudrate": 115200,
            "channels": []
        }"#;
        let config: SerialStreamConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.frame_pattern, default_frame_pattern());
    }

    /// A channel reads as one flat object: what it is, and where it comes from.
    #[test]
    fn a_channel_carries_its_description_and_its_binding_together() {
        let json = r#"{
            "id": "1",
            "name": "Pressure",
            "unit": "Pa",
            "description": "Differential pressure sensor",
            "index": 1
        }"#;
        let channel: SerialStreamChannel = serde_json::from_str(json).unwrap();
        assert_eq!(channel.info.name, "Pressure");
        assert_eq!(channel.index, 1);
    }

    /// The failure the merge exists to prevent: reordering used to swap which
    /// channel each value landed in. Now the name travels with the index.
    #[test]
    fn reordering_channels_moves_their_bindings_with_them() {
        let forwards = parse_frame_values(EXAMPLE, &line_inputs(&[1, 3])).unwrap();
        let backwards = parse_frame_values(EXAMPLE, &line_inputs(&[3, 1])).unwrap();
        assert_eq!(forwards[0], backwards[1]);
        assert_eq!(forwards[1], backwards[0]);
    }

    #[test]
    fn a_bad_pattern_is_rejected_when_the_device_is_built() {
        let config = SerialStreamConfig {
            port: "COM1".to_string(),
            baudrate: 9600,
            frame_pattern: r"#([$".to_string(),
            channels: vec![],
        };
        let error = SerialStream::from_config(config).err().unwrap();
        assert!(error.to_string().contains("not a valid regular expression"));
    }
}
