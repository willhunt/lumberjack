export interface ConfigStore {
    devices: {[name: string]: Device},
    dashboards: Dashboard[],
}
export interface Device {
    name: string,
    plugin: string,
    channels: Channel[]
}
export interface Channel {
    name: string,
    units: string,
    active: Boolean,
}

export interface Widget {
    name: string,
    channels: string[],
}
export interface Dashboard {
    name: string, 
    widgets: Widget[]
}

// referenced by <device name>.<channel name>
export interface DataLog {
    [name: string]: {[name: string]: number[]}[]
}