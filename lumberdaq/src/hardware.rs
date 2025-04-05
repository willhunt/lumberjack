// Add module 'hardware' in hardware folder
pub mod mock_hardware;
pub mod ni_usb6001;
// pub mod firmata;
use crate::mock_hardware::MockConfig;
use crate::ni_usb6001::NiUsb6001Config;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub enum Device {
    mock_harware,
    ni_usb6001,
}