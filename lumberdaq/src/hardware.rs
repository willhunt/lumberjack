// Add module 'hardware' in hardware folder
pub mod mock_hardware;
pub mod ni_usb6001;
use crate::datapoint::DataPoint;
// pub mod firmata;
use crate::Result;
use mock_hardware::{ MockHardware, MockHardwareInput };
use ni_usb6001::{ NiUsb6001, NiUsb6001Input };
use crate::device::DeviceInterface;
use serde::{ Deserialize, Serialize };

pub trait HardwareDataAquisition {
    fn read(&mut self) -> Result<Vec<Vec<DataPoint>>>;
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "type")]  // Adds "type: MockHardware" identifies to serilaized output, https://serde.rs/enum-representations.html
pub enum Hardware {
    MockHardware(MockHardware),
    NiUsb6001(NiUsb6001),
    None,
}
impl HardwareDataAquisition for Hardware {
    fn read(&mut self) -> Result<Vec<Vec<DataPoint>>> {
        match self {
            Hardware::MockHardware(device) => {
                device.read()
            },
            Hardware::NiUsb6001(device) => {
                device.read()
            },
            Hardware::None => Err("No hardware is available for this device. Typically this type is used for reading data.".into())
        }
    }
}
impl DeviceInterface for Hardware {
    fn connect(&mut self) -> Result<()> {
        match self {
            Hardware::MockHardware(device) => {
                device.connect()?;
            },

            Hardware::NiUsb6001(device) => {
                device.connect()?
            },
            Hardware::None => return Err("No hardware is available for this device. Typically this type is used for reading data.".into())
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
pub enum Input {
    MockHardware(MockHardwareInput),
    NiUsb6001(NiUsb6001Input),
}