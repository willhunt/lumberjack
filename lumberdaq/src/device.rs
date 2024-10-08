use crate::channel::Channel;

pub trait DeviceDataAquisition {
    fn connect(&self);
    fn read(&self, channels: &mut Vec<Channel>) {
        for channel in channels.iter_mut() {
            match channel.read() {
                Err(e) => println!("{e}"),
                Ok(()) => (),
            }
        }
    }
}

// #[allow(dead_code)]
// pub enum DeviceType {
//     Usb {
//         baudrate: i32,
//     },
//     Mock,
// }

pub struct Device {
    pub name: String,
    pub description: String,
    pub channels: Vec<Channel>,
    // pub device_type: DeviceType,
    pub device_config: Box<dyn DeviceDataAquisition>,
}

impl Device {
    pub fn add_channel(&mut self, channel: Channel) {
        self.channels.push(channel);
    }

    pub fn print_latest(&self) {
        println!("Latest reading from device: {}", &self.name);
        for channel in &self.channels {
            println!("    {}", channel.channel_data.latest_as_string());
        }
    }

    pub fn check_and_write(&mut self) {
        for channel in &mut self.channels {
            channel.channel_data.check_and_write();
        }
    }

    // pub fn add_to_hdf(&self, file: )

    // Moved this to trait default. Leaving for now, if other strategy works, delete.
    // pub fn read(&mut self) {
    //     for channel in &mut self.channels {
    //         match channel.read() {
    //             Err(e) => println!("{e}"),
    //             Ok(()) => (),
    //         }
    //     }
    // }
}