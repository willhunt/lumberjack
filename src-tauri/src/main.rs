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

struct PluginState(hardware_plugins::plugin_common::PluginManager);

fn main() {
    // let config_path = Path::new("./examples/mock.json");
    // let result = load::load_config(String::from("./mock_config.json"));

    // Plugins
    // let plugin_manager = plugins::find_hardware_plugins();


    tauri::Builder::default()
        .manage(PluginState(plugins::find_hardware_plugins()))
        .invoke_handler(tauri::generate_handler![
            hardware_serial::list_serial_ports,
            plugins::list_hardware_plugins,
            // plugins::read_hardware,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
