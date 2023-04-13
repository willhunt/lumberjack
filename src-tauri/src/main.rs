#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]
// #[macro_use(c)]
// extern crate cute;

mod hardware_serial;
mod hardware_plugins;
mod plugins;
mod load;

// use std::path::Path;

fn main() {
    // let config_path = Path::new("./examples/mock.json");
    // let result = load::load_config(String::from("./mock_config.json"));

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![hardware_serial::list_serial_ports])
        .invoke_handler(tauri::generate_handler![plugins::list_hardware_plugins])
        // .invoke_handler(tauri::generate_handler![plugins::read_hardware])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
