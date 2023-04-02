use serde::{Deserialize, Serialize};
use serde_json;

#[derive(Serialize, Deserialize)]
struct Configuration {
    devices: Vec<Device>,
    layouts: Vec<Layout>,
}

#[derive(Serialize, Deserialize)]
struct Device {
    name: String,
    channels: Vec<Channel>,

}

#[derive(Serialize, Deserialize)]
struct Channel {
    name: String,
    active: bool,
    units: String
}

#[derive(Serialize, Deserialize)]
struct Layout {
    name: String,
    widgets: Vec<Widget>,
}

#[derive(Serialize, Deserialize)]
struct Widget {
    name: String,
    channels: String,
}

pub fn load_config(file_name: String) -> serde_json::Result<()> {
    let str_data = std::fs::read_to_string(file_name)
        .expect("Error reading config file");
    let config: Configuration = serde_json::from_str(&str_data)?;
    return Ok(());
}
