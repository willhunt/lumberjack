use crate::datapoint::DataPoint;

const N_DATAPOINTS: usize = 10;
const WRITE_INTERVAL: chrono::TimeDelta = chrono::TimeDelta::seconds(60);

pub enum WriteStatus {
    YesToWrite,
    NoToWrite,
    WriteComplete,
}

pub struct Channel {
    pub name: String,
    pub subport: String,
    pub unit: String,
    datapoints: [Option<DataPoint>; N_DATAPOINTS],//Vec<DataPoint>,
    pub datapoint_last: Option<DataPoint>,
    datapoint_index: usize,
    last_write: chrono::DateTime<chrono::Utc>,
}

impl Channel {
    pub fn new(name: String, subport: String, unit: String) -> Channel {
        Channel {
            name: name,
            subport: subport,
            unit: unit,
            datapoints: [None; N_DATAPOINTS],
            datapoint_last: None,
            datapoint_index: 0,
            last_write: chrono::Utc::now()
        }
    }

    pub fn add_datapoint(&mut self, datapoint: DataPoint) -> Result<(), &'static str> {
        if self.datapoint_index > N_DATAPOINTS {
            return Err("The store of 'datapoints' is full, write to file before adding more.");
        }
        self.datapoints[self.datapoint_index] = Some(datapoint);
        self.datapoint_last = Some(datapoint);
        self.datapoint_index += 1;  // Move on to next index
        return Ok(());
    }   

    pub fn latest_as_string(&self) -> String {
        match self.datapoint_last {
            Some(data) => format!("{} ({}): {}, {}{}", self.name, self.datapoint_index, data.datetime, data.value, self.unit),
            None => format!("{}: No data", self.name)
        }
    }

    /// Checks if the data should be written to file or not based upon the number of datapoints and the time elapsed after the previous write.
    pub fn check_and_write(&mut self) -> WriteStatus {
        if self.datapoint_index == 0 {
            return WriteStatus::NoToWrite;
        }
        if self.datapoint_index >= N_DATAPOINTS {
            return self.write();
        }
        if (chrono::Utc::now() - self.last_write) > WRITE_INTERVAL {
            return self.write();
        }
        WriteStatus::NoToWrite
    }

    pub fn write(&mut self) -> WriteStatus {


        // Clear datapoints and reset index
        self.datapoint_index = 0;
        self.datapoints = [None; N_DATAPOINTS];
        self.last_write = chrono::Utc::now();
        WriteStatus::WriteComplete
    }
}