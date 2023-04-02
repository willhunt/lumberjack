export interface ConfigStore {
    devices: Device[],
    dashboards: Dashboard[],
}
export interface Device {
    name: String,
    plugin: String,
    active: Boolean,
    units: string
}
export interface Widget {
    name: String, 
    widgets: {
        name: String,
        channels: String[]
    }
}
export interface Dashboard {
    name: String, 
    widgets: Widget[]
}