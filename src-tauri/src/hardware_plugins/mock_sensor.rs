use crate::hardware_plugins::plugin_common::{ GetInfo };

pub struct MockSensor {
    pub frequency: u32,
}

// impl MockSensor {
//     const NAME: String = String::from("Mock Sensor");
// }

impl GetInfo for MockSensor {
    fn get_name(&self) -> String {
        // self.NAME.clone()  // not sure if this is good practise...can check later with more knowledge
        String::from("Mock Sensor")
    }
}
