use std::time::Instant;
use crate::hardware_plugins::plugin_common::{ Channel, HardwareInterface};

pub struct MockSensor {
    frequency: u32,
    init_time: Instant,
}

impl HardwareInterface for MockSensor {
    fn get_name(&self) -> String {
        // self.NAME.clone()  // not sure if this is good practise...can check later with more knowledge
        return String::from("MockSensor");
    }
    fn initialize(&self) -> bool {
        // self.init_time = Instant::now();
        return true;
    }
    fn read_channels(&self) ->  Vec<Channel> {
        // Dummy channel readings of sine and cosine
        let elapsed_time = self.init_time.elapsed().as_secs() as f64;
        let channel1 = Channel {
            name: String::from("Analog 1"),
            device: self.get_name(),
            reading: elapsed_time.sin(),
        };
        let channel2 = Channel {
            name: String::from("Analog 1"),
            device: self.get_name(),
            reading: elapsed_time.cos(),
        };
        return vec![channel1, channel2];
    }
}

// impl MockSensor {
// }

pub fn build_plugin() -> MockSensor {
    let plugin = MockSensor {
        frequency: 500,
        init_time: Instant::now(),
    };
    return plugin;
}