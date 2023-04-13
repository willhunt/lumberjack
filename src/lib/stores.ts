import { writable } from 'svelte/store';
import type { Writable } from 'svelte/store';
import type { ConfigStore, Device, Dashboard, DataLog } from '$lib/types'

export const config_store: Writable<ConfigStore> = writable({
    devices: {},
    dashboards: [],
})

// class ConfigStore {
//     constructor
// }

// Navigation bar width
export let nav_width_store = writable(300)

// Recorded data
export let logged_data: Writable<DataLog> = writable({})
