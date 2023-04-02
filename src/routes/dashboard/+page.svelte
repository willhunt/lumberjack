<script>
    // import HardwarePluginArduino from '$lib/hardware-plugins/HardwarePluginArduino.svelte'
    import { onMount } from 'svelte'
    import { invoke } from '@tauri-apps/api/tauri'
    import { resourceDir } from '@tauri-apps/api/path';

    function goHome() {
        location.href="/"
    }

    async function listDashboards() {

    }
    let dashboards = ["None"]
    // let hardware_plugins_short = ["None"]
    let active_dashboard = ""

    onMount(async () => {
		listDashboards()
        // hardware_plugins_short = hardware_plugins.map(x => {x = x.replace("HardwarePlugin", ""); return x})
        const resourceDirPath = await resourceDir();
        console.log(resourceDirPath)
	});
    
</script>

<style>
    
</style>

<button class="page-button" on:click={goHome}><span class="material-icons">arrow_circle_left</span></button>
<h1>Add dashboard</h1>
<div class="form-line">
    <label>
        Dashboard configuration: 
        <select name="plugin-selction" bind:value={active_dashboard}>
            {#each dashboards as dashboard, i}
                <option value="{dashboard}">
                    {dashboard}
                </option>
            {/each}
        </select>
    </label>
</div>

