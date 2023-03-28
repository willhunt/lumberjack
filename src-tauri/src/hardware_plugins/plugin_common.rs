// pub trait Initialize {
//     fn initialize(&self);
// }
pub trait GetInfo {
    fn get_name(&self) -> String;
}

pub struct PluginManager {
    // pub plugins_initialize: Vec<Box<dyn Initialize>>,
    pub plugins: Vec<Box<dyn GetInfo>>,
}

impl PluginManager {
    pub fn get_info(&self) -> Vec<String> {
        let mut plugin_names = Vec::new();

        for plugin in self.plugins.iter() {
            plugin_names.push(plugin.get_name());
        }
        // plugin_names.push(String::from("Test"));
        plugin_names
    }
}