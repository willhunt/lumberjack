use crate::device::DataAquisition;

pub struct Daq {
    pub devices: Vec<Box<dyn DataAquisition>>,
}