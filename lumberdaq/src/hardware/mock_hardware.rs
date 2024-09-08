use crate::devices::{ Channel, Device, DeviceType, DataPoint };
use serialport;
use chrono;
use rand::random;

pub fn create_device() -> Device {
    let port = serialport::SerialPortInfo {
        port_name: "COMx".to_string(),
        port_type: serialport::SerialPortType::Unknown
    };
    let channel1 = Channel {
        name: "contant".to_string(),
        subport: "a0".to_string(),
        unit: "-".to_string(),
        datapoints: Vec::new(),
    };
    let channel2 = Channel {
        name: "random".to_string(),
        subport: "a1".to_string(),
        unit: "-".to_string(),
        datapoints: Vec::new(),
    };
    Device {
        name: "Mock device".to_string(),
        port: port,
        channels: vec![channel1, channel2],
        device_type: DeviceType::Mock,
    }
}

pub fn read(device: &mut Device) {
    for channel in &mut device.channels {
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
        channel.datapoints.push(datapoint);
    }
    
}

pub fn print_latest(device: &Device) {
    println!("Latest reading from device: {}", device.name);
    for channel in &device.channels {
        if let Some(datapoint) = channel.datapoints.last() {
            println!("    {}: {}, {}", channel.name, datapoint.datetime, datapoint.value);
        } else {
            println!("No data");
        }
        
    }
}
