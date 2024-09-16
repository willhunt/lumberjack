

pub trait DataAquisition {
    fn read(&mut self);
    fn print_latest(&self);
}


pub struct Daq {
    pub devices: Vec<Box<dyn DataAquisition>>,
}