use chrono;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Copy, Clone)]
pub struct DataPoint {
    pub datetime: chrono::DateTime<chrono::Utc>,
    pub value: f64,
}