<script>
    // import HardwarePluginArduino from '$lib/hardware-plugins/HardwarePluginArduino.svelte'
    import { onMount } from 'svelte'
    import { invoke } from '@tauri-apps/api/tauri'
    import { resourceDir } from '@tauri-apps/api/path';
    import { goto } from '$app/navigation'
    import { config_store } from '$lib/stores';


    export let data // Data from slug


</script>

<style>
    
</style>

<button class="page-button" on:click={() => goto("/")}><span class="material-icons">arrow_circle_left</span></button>
<h1>{$config_store.dashboards[Number(data.slug)].name}</h1>
<div class="form-line">
    Dashboard configuration:
    Edit text file please!
</div>

