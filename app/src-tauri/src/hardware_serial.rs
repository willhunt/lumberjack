use serialport;
// use serde::ser::{Serialize, Serializer, SerializeStruct};
use serde::Serialize;

#[derive(Serialize)] // Required for #[tauri::command]
pub struct PortDetail {
    name: String,
    product: String,
    port_type: String,
}

// This is the chumps way to do it instead of #[derive(Serialize)] .......chummmmmmmmmmp
// impl Serialize for PortDetail {
//     // Required for #[tauri::command], must be able to serialize data types to send to frontend. https://serde.rs/impl-serialize.html
//     fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
//     where
//         S: Serializer,
//     {
//         // 3 is the number of fields in the struct.
//         let mut state = serializer.serialize_struct("PortDetail", 3)?;
//         state.serialize_field("name", &self.name)?;
//         state.serialize_field("product", &self.product)?;
//         state.serialize_field("port_type", &self.port_type)?;
//         state.end()
//     }
// }

#[tauri::command]
pub fn list_serial_ports() -> Vec<PortDetail> {
    let ports = serialport::available_ports().expect("No ports found!");
    
    // let port_names = c![port.port_name, for port in ports];

    // Test struct with dummy values:
    // let port_details = vec![PortDetail{name:String::from("COM1"), product:String::from("Arduino"), port_type:String::from("USB")}];

    let mut port_details: Vec<PortDetail> = Vec::new();

    for port in ports {
        let mut port_detail = PortDetail{
            name: port.port_name,
            product: String::from("None"),
            port_type: String::from("None"),
        };
        match port.port_type {
            serialport::SerialPortType::UsbPort(info) => {
                port_detail.port_type = String::from("USB");
                port_detail.product = info.product.as_ref().map_or(String::from("-"), String::from);
            }
            serialport::SerialPortType::BluetoothPort => {
                port_detail.port_type = String::from("Bluetooth");
            }
            serialport::SerialPortType::PciPort => {
                port_detail.port_type = String::from("PCI");
            }
            serialport::SerialPortType::Unknown => {
                port_detail.port_type = String::from("-");
            }
        }
        port_details.push(port_detail);
    }
    // port_names
    port_details
}