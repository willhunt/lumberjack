<script lang="ts">
    // import HardwarePluginArduino from '$lib/hardware-plugins/HardwarePluginArduino.svelte'
    import { onMount } from 'svelte'
    import { invoke } from '@tauri-apps/api/tauri'
    import type { ConfigStore } from '$lib/types'
    import { config_store } from '$lib/stores';
	import { goto } from '$app/navigation';

    let config_text = ""

    config_store.subscribe((value: ConfigStore) => {
        config_text = JSON.stringify(value, null, 2)
    })

    onMount(async () => {
        
	});
    
    
</script>

<style>
    .code-block {

    }
</style>

<button class="page-button" on:click={() => goto("/")}><span class="material-icons">arrow_circle_left</span></button>
<h1>Configuration</h1>
<pre class="code-block"><code>{config_text}</code></pre>

