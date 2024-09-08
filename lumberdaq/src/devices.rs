use serialport;
use chrono;

pub struct DataPoint {
    pub datetime: chrono::DateTime<chrono::Utc>,
    pub value: f64,
}

pub struct Channel {
    pub name: String,
    pub subport: String,
    pub unit: String,
    pub datapoints: Vec<DataPoint>,
}

#[allow(dead_code)]
pub enum DeviceType {
    Usb {
        baudrate: i32,
    },
    Mock,
}

pub struct Device {
    pub name: String,
    pub port: serialport::SerialPortInfo,
    pub channels: Vec<Channel>,
    pub device_type: DeviceType,
}