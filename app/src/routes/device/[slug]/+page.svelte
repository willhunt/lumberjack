<script lang="ts">
    // import HardwarePluginArduino from '$lib/hardware-plugins/HardwarePluginArduino.svelte'
    import { onMount } from 'svelte'
    import { invoke } from '@tauri-apps/api/tauri'
    import { goto } from '$app/navigation'
    import { config_store } from '$lib/stores';

    export let data // Data from slug
    let hardware_plugins = ["None"]
    // let hardware_plugins_short = ["None"]
    let active_plugin = ""

    async function listHardwarePlugins() {
        // const file_imports = import.meta.glob("$lib/hardware-plugins/HardwarePlugin*.svelte")  // https://vitejs.dev/guide/features.html#glob-import
        // let plugin_names = []
        // let plugin_shortnames = []
        // for (const path in file_imports) {
        //     let file_name = path.split("/").pop()
        //     if (file_name) {  // Required as file_name may be undefined, (typescript)
        //         let plugin_name = file_name.split(".")[0]
        //         plugin_names.push(plugin_name)
        //         // Import plugins dynamically
        //         file_imports[path]()
        //     }
        // }
        hardware_plugins = await invoke('list_hardware_plugins')
        // invoke('list_hardware_plugins').then((plugins: any) => {
        //     hardware_plugins = plugins
        // })
    }
    
    onMount(async () => {
        // Get device index

		listHardwarePlugins()
        // hardware_plugins_short = hardware_plugins.map(x => {x = x.replace("HardwarePlugin", ""); return x})
        console.log("Device loaded: " + data.slug)
	});
</script>

<style>
    
</style>

<button class="page-button" on:click={() => goto("/")}><span class="material-icons">arrow_circle_left</span></button>
<h1>{$config_store.devices[data.slug].name}</h1>
<div class="form-line">
    <label>
        Available hardware plugin: 
        <select name="plugin-selction" bind:value={active_plugin}>
            {#each hardware_plugins as hardware_plugin, i}
                <option value="{hardware_plugin}">
                    {hardware_plugin}
                </option>
            {/each}
        </select>
    </label>
    <div>
        Channels:
        {#each $config_store.devices[data.slug].channels as channel}
            <li>{channel.name}</li>
        {/each}
    </div>
</div>
{active_plugin}
<!-- <{active_plugin} /> -->