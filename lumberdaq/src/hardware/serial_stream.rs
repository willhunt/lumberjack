use crate::Result;
use crate::datapoint::DataPoint;
use crate::channel::{ Channel, ChannelDataAquisition };
use crate::device::{ Device, DeviceInterface };
use crate::hardware::{HardwareDataAquisition, Hardware };
use serde::{ Deserialize, Serialize };
use serialport;
use chrono;
use std::time::Duration;

/// Everything needed to describe a serial device in a config file.
#[derive(Serialize, Deserialize, Clone)]
pub struct SerialStreamConfig {
    pub description: String,
    pub port: String,
    pub baudrate: u32,
    pub inputs: Vec<SerialStreamInput>,
}

/// The running device: its settings, plus the open port once connected.
///
/// The port handle is the reason this type cannot derive Serialize or Clone,
/// and the reason the settings live in a separate struct that can.
pub struct SerialStream {
    config: SerialStreamConfig,
    serial_port: Option<Box<dyn serialport::SerialPort + Send>>,
}

impl SerialStream {
    pub fn new(port: String, baudrate: u32) -> Result<SerialStream> {
        SerialStream::from_config(SerialStreamConfig {
            description: "Device streaming over serial.".to_string(),
            port: port,
            baudrate: baudrate,
            inputs: vec![],
        })
    }

    pub fn from_config(config: SerialStreamConfig) -> Result<SerialStream> {
        Ok(SerialStream {
            config: config,
            serial_port: None,
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


impl HardwareDataAquisition for SerialStream {
    fn read(&mut self) -> Result<Vec<Vec<DataPoint>>> {
        let readings: Vec<Vec<DataPoint>> = vec![];
        let _timestamp = chrono::Utc::now();
        /////////////////////////////////////////////// TODO
        Ok(readings)
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
