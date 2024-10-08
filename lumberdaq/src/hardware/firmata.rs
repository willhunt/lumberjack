use crate::datapoint::DataPoint;
use crate::channel::Channel;
use crate::device::{ Device, DeviceType, DataAquisition };

pub fn create_device() -> Device {
    let port = serialport::SerialPortInfo {
        port_name: "COM3".to_string(),
        port_type: serialport::SerialPortType::Unknown
    };
    Device {
        name: "Firmata device".to_string(),
        port: port,
        channels: Vec::new(),
        device_type: DeviceType::Usb {
            baudrate: 115200
        },
    }
}

