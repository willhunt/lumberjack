

pub trait DataAquisition {
    fn read(&mut self);
}


pub struct Daq {
    pub devices: Vec<Box<dyn DataAquisition>>,
}