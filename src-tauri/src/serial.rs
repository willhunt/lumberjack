use serialport::*;

#[tauri::command]
pub fn list_serial_ports() -> Vec<String> {
    let ports = serialport::available_ports().expect("No ports found!");
    // vec!["COM1".to_string(), "COM2".to_string()]
    let port_names = c![port.port_name, for port in ports];
    port_names
}