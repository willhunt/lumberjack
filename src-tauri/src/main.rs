#![cfg_attr(
  all(not(debug_assertions), target_os = "windows"),
  windows_subsystem = "windows"
)]
#[macro_use(c)]
extern crate cute;

mod serial;

fn main() {
  tauri::Builder::default()
    .invoke_handler(tauri::generate_handler![serial::list_serial_ports])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}

#[tauri::command]
fn list_serial_ports_dummy() -> Vec<String> {
    vec!["COM1".to_string(), "COM2".to_string()]
}