use crate::device::Device;

pub struct Daq {
    pub devices: Vec<Device>,
}
impl Daq {
    pub fn connect(&self) {
        for device in self.devices.iter() {
            device.device_config.connect();
        }
    }
}