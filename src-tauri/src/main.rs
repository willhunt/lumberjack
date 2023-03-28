#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]
// #[macro_use(c)]
// extern crate cute;

mod hardware_serial;
mod hardware_plugins;
mod plugins;

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![hardware_serial::list_serial_ports])
        .invoke_handler(tauri::generate_handler![plugins::list_hardware_plugins])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
