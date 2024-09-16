use serialport;
use chrono;

const N_DATAPOINTS: usize = 1000;
const WRITE_INTERVAL: chrono::TimeDelta = chrono::TimeDelta::new(10, 0).unwrap();

#[derive(Copy)]
#[derive(Clone)]
pub struct DataPoint {
    pub datetime: chrono::DateTime<chrono::Utc>,
    pub value: f64,
}

pub struct Channel {
    pub name: String,
    pub subport: String,
    pub unit: String,
    datapoints: [Option<DataPoint>; N_DATAPOINTS],//Vec<DataPoint>,
    datapoint_last: Option<DataPoint>,
    datapoint_index: usize,
    last_write: chrono::DateTime<chrono::Utc>,
}

impl Channel {
    pub fn new(name: String, subport: String, unit: String) -> Channel {
        Channel {
            name: name,
            subport: subport,
            unit: unit,
            datapoints: [None; 1000],
            datapoint_last: None,
            datapoint_index: 0,
            last_write: chrono::Utc::now()
        }
    }

    pub fn add_datapoint(&self, datapoint: DataPoint) {
        if self.datapoint_index > N_DATAPOINTS {
            self.write_to_file();
        }
        self.datapoints[self.datapoint_index] = Some(datapoint);
        self.datapoint_last = Some(datapoint);
        self.datapoint_index += 1;  // Move on to next index
    }

    /// Checks if the data should be written to file or not based upon the number of datapoints and the time elapsed after the previous write.
    pub fn write_check(&self) -> bool {
        if self.datapoint_index > N_DATAPOINTS {
            return true;
        }
        if (chrono::Utc::now() - self.last_write) > WRITE_INTERVAL {
            return true;
        }
        return false;
    }

    pub fn write_to_file(&self) {


        // Clear datapoints and reset index
        self.datapoint_index = 0;
        self.datapoints = [None; 1000];
    }
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

impl Device {
    fn print_latest(&self) {
        println!("Latest reading from device: {}", &self.name);
        for channel in &self.channels {
            if let Some(datapoint) = channel.datapoint_last {
                println!("    {}: {}, {}", channel.name, datapoint.datetime, datapoint.value);
            } else {
                println!("No data");
            }
        }
    }
}