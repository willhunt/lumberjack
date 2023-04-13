use crate::hardware_plugins::plugin_common::{ PluginManager, Channel };
use crate::hardware_plugins::mock_sensor;


#[tauri::command]
pub fn list_hardware_plugins() -> Vec<String> {
    let plugin_manager = find_hardware_plugins();
    let plugin_names = plugin_manager.get_plugin_names(); //vec![String::from("Dummy")];
    return plugin_names;
}

// #[tauri::command]
// pub fn read_hardware() -> Vec<Channel> {
//     let plugin_manager = find_hardware_plugins();
//     let channels = plugin_manager.read_hardware(); //vec![String::from("Dummy")];
//     return channels;
// }

fn find_hardware_plugins() -> PluginManager {
    let plugin_manager = PluginManager {
        plugins: vec![
            Box::new(mock_sensor::build_plugin())
        ],
    };
    return plugin_manager;
}
