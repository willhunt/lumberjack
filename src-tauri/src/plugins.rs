use crate::hardware_plugins::plugin_common::{ PluginManager };
use crate::hardware_plugins::mock_sensor::{ MockSensor };


#[tauri::command]
pub fn list_hardware_plugins() -> Vec<String> {
    let plugin_manager = init_hardware_plugins();
    // Do this dumbly for now
    let plugin_names = plugin_manager.get_info(); //vec![String::from("DDummy")];
    plugin_names
}

fn init_hardware_plugins() -> PluginManager {
    let plugin_manager = PluginManager {
        plugins: vec![
            Box::new(MockSensor {
                frequency: 100,
            })
        ],
    };
    plugin_manager
}
