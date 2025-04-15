use crate::Result;
use crate::datapoint::DataPoint;
use crate::channel::{ Channel, ChannelDataAquisition };
use crate::device::{ Device, DeviceInterface };
use crate::hardware::{HardwareDataAquisition, Hardware };
use serde::{ de, Deserialize, Serialize };
use serialport;
use chrono;
use std::io::{self, Read};
use std::time::Duration;

#[derive(Serialize, Deserialize)]  // Cloning an issue with serialport
pub struct SerialStream {
    description: String,
    inputs: Vec<SerialStreamInput>,
    #[serde(skip, default)]
    serial_port: Option<Box<dyn serialport::SerialPort + Send>>,
    port: String,
    baudrate: u32,
    // #[serde(skip)]
    // serial: Option<>,
}

impl DeviceInterface for SerialStream {
    fn connect(&mut self) -> Result<()> {
        let port = serialport::new(&self.port, self.baudrate)
            .timeout(Duration::from_millis(100))
            .open()?;
        Ok(())
    }
}


impl HardwareDataAquisition for SerialStream {
    fn read(&mut self) -> Result<Vec<Vec<DataPoint>>> {
        let mut readings: Vec<Vec<DataPoint>> = vec![];
        let timestamp = chrono::Utc::now();
        /////////////////////////////////////////////// TODO
        Ok(readings)
    }
}

impl SerialStream {
    pub fn new(port: String, baudrate: u32) -> Result<SerialStream> {
        Ok(SerialStream {
            description: "Device streaming over serial.".to_string(),
            inputs: vec![],
            port: port,
            baudrate: baudrate,
            serial_port: None,
        })
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub enum SerialStreamInput {
    LineInput { index: i64 },
}
impl ChannelDataAquisition for SerialStreamInput {
    fn read(&mut self) -> Result<Vec<DataPoint>> {
        match self {
            SerialStreamInput::LineInput {index: i64} => {
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
            hardware.inputs.push(SerialStreamInput::LineInput { index: index });
        },
        _ => {
            return Err("This channel can only be added to a NI USB 6001 device.".into())
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
