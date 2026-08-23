use serde::Serialize;

pub trait HardwareInterface {
    fn get_name(&self) -> String;
    fn initialize(&self) -> bool;
    fn read_channels(&self) -> Vec<Channel>;
}

#[derive(Serialize)] // Required for #[tauri::command]
pub struct Channel {
    pub name: String,
    pub device: String,
    pub reading: f64,
}

pub struct PluginManager {
    // pub plugins_initialize: Vec<Box<dyn Initialize>>,
    pub plugins: Vec<Box<dyn HardwareInterface>>,
}

impl PluginManager {
    pub fn get_plugin_names(&self) -> Vec<String> {
        let mut plugin_names = Vec::new();
        for plugin in self.plugins.iter() {
            plugin_names.push(plugin.get_name());
        }
        // plugin_names.push(String::from("Test"));
        return plugin_names;
    }
    pub fn initialize_hardware(&self) -> Vec<bool> {
        let mut responses = Vec::new();
        for plugin in self.plugins.iter() {
            responses.push(plugin.initialize());
        }
        return responses;
    }
    pub fn read_hardware(&self) -> Vec<Channel> {
        let mut channels = Vec::new();
        for plugin in self.plugins.iter() {
            // Using append here moves elements of vector requiring it to be mut, extend does not.
            channels.extend(plugin.read_channels());
        }
        return channels;
    }
}