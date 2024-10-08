use crate::device::Device;
use crate::storage::{ create_hdf_file, add_hdf_device, add_hdf_channel };
// use hdf5;

pub struct Daq {
    pub devices: Vec<Device>,
    hdf_path: std::path::PathBuf,
}
impl Daq {
    pub fn new(devices: Vec<Device>, storage_path: std::path::PathBuf) -> Daq {
        Daq {
            devices: devices,
            hdf_path: storage_path,
        }
    }
    
    pub fn connect(&self) {
        for device in self.devices.iter() {
            device.device_config.connect();
        }
    }

    pub fn init_storage(&mut self) -> Result<(), hdf5::Error> {
        let hdf_file = create_hdf_file(&self.hdf_path)?;
        // Add devices
        for device in self.devices.iter() {
            let device_group = add_hdf_device(&hdf_file, &device.name)?;
            for channel in device.channels.iter() {
                add_hdf_channel(
                    &device_group,
                    channel.channel_data.name.as_str(),
                    channel.channel_data.unit.as_str(),
                    channel.channel_data.description.as_str()
                )?;
            }
        }
        return Ok(());
    }
}