use crate::datapoint::DataPoint;
use crate::channel::ChannelData;
use crate::device::{ Device, DeviceType, DataAquisition };
use serialport;
use chrono;
use rand::random;

pub struct MockDevice {
    pub device: Device
}

impl DataAquisition for MockDevice {
    fn read(&mut self) {
        for channel in &mut self.device.channels {
            let datapoint: DataPoint;
            if channel.name.contains("random") {
                datapoint = DataPoint {
                    datetime: chrono::Utc::now(),
                    value: random(),
                };
            }
            else {
                datapoint = DataPoint {
                    datetime: chrono::Utc::now(),
                    value: 100.0,
                };
            }
            match channel.add_datapoint(datapoint) {
                Err(e) => println!("{e}"),
                Ok(()) => (),
            }
        }
    }

    fn check_and_write(&mut self) {
        self.device.check_and_write();
    }

    fn print_latest(&self) {
        self.device.print_latest();
    }
}

pub struct RandomChannel {
    channel_data: ChannelData,
}
impl RandomChannel {
    pub fn read(&self) -> DataPoint {
        DataPoint {
            datetime: chrono::Utc::now(),
            value: random(),
        }
    }
}

impl MockDevice {
    pub fn new() -> MockDevice {
        let port = serialport::SerialPortInfo {
            port_name: "COMx".to_string(),
            port_type: serialport::SerialPortType::Unknown
        };
        let channeldata1 = ChannelData::new("contant".to_string(), "a0".to_string());
        let channeldata2 = ChannelData::new("random".to_string(), "a1".to_string());
    
        let device = Device {
            name: "Mock device".to_string(),
            port: port,
            channels: vec![channeldata1, channeldata2],
            device_type: DeviceType::Mock,
        };
        MockDevice {
            device: device
        }
    }
}




// pub fn create_device() -> MockDevice {
//     let port = serialport::SerialPortInfo {
//         port_name: "COMx".to_string(),
//         port_type: serialport::SerialPortType::Unknown
//     };
//     let channel1 = Channel::new("contant".to_string(), "a0".to_string(), "-".to_string());
//     let channel2 = Channel::new("random".to_string(), "a1".to_string(), "-".to_string());

//     let device = Device {
//         name: "Mock device".to_string(),
//         port: port,
//         channels: vec![channel1, channel2],
//         device_type: DeviceType::Mock,
//     };
//     MockDevice {
//         device: device
//     }
// }
