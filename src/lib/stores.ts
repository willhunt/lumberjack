import { writable } from 'svelte/store';
import type { Writable } from 'svelte/store';
import type { ConfigStore, Device, Dashboard } from '$lib/types'

export const config_store: Writable<ConfigStore> = writable({
    devices: [],
    dashboards: [],
})

// class ConfigStore {
//     constructor
// }