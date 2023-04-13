<script lang="ts">
    // import HardwarePluginArduino from '$lib/hardware-plugins/HardwarePluginArduino.svelte'
    import { onMount } from 'svelte'
    import { invoke } from '@tauri-apps/api/tauri'
    import type { ConfigStore } from '$lib/types'
    import { config_store } from '$lib/stores';

    export let channel: string
    let value = 0;

    config_store.subscribe((value: ConfigStore) => {

    })

    onMount(async () => {
        
	});
    
    
</script>

<style>
    .widget {
        border-style: solid;
        border-radius: 5px;
        padding: 5px;
        margin: 5px;
    }
    .numeric-display {
        font-size: large;
    }
</style>

<div class="widget">
    <div class="numeric-display">{value}</div>
    <div class="channel">({channel})</div>
</div>

