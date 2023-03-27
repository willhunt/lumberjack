<script>
    // import HardwarePluginArduino from '$lib/hardware-plugins/HardwarePluginArduino.svelte'
    import { onMount } from 'svelte'

    function goHome() {
        location.href="/"
    }

    function listHardwarePlugins() {
        const file_imports = import.meta.glob("$lib/hardware-plugins/HardwarePlugin*.svelte")  // https://vitejs.dev/guide/features.html#glob-import
        let plugin_names = []
        let plugin_shortnames = []
        for (const path in file_imports) {
            let file_name = path.split("/").pop()
            if (file_name) {  // Required as file_name may be undefined, (typescript)
                let plugin_name = file_name.split(".")[0]
                plugin_names.push(plugin_name)
                // Import plugins dynamically
                file_imports[path]()
            }
        }
        return plugin_names
    }
    let hardware_plugins = ["None"]
    let hardware_plugins_short = ["None"]
    let active_plugin = ""
    onMount(async () => {
		hardware_plugins = listHardwarePlugins()
        hardware_plugins_short = hardware_plugins.map(x => {x = x.replace("HardwarePlugin", ""); return x})
	});
</script>

<style>
    
</style>

<button class="page-button" on:click={goHome}><span class="material-icons">arrow_circle_left</span></button>
<h1>Add device</h1>
<div class="form-line">
    <label>
        Hardware plugin: 
        <select name="plugin-selction" bind:value={active_plugin}>
            {#each hardware_plugins as hardware_plugin, i}
                <option value="{hardware_plugins}">
                    {hardware_plugins_short[i]}
                </option>
            {/each}
        </select>
    </label>
</div>
{active_plugin}
<!-- <{active_plugin} /> -->