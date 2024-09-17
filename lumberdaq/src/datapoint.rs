use chrono;

#[derive(Copy)]
#[derive(Clone)]
pub struct DataPoint {
    pub datetime: chrono::DateTime<chrono::Utc>,
    pub value: f64,
}