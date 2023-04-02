<script lang="ts">
    // import HardwarePluginArduino from '$lib/hardware-plugins/HardwarePluginArduino.svelte'
    import { onMount } from 'svelte'
    import { invoke } from '@tauri-apps/api/tauri'
    import type { ConfigStore } from '$lib/types'
    import { config_store } from '$lib/stores';

    let config_text = ""

    function goHome() {
        location.href="/"
    }
    config_store.subscribe((value: ConfigStore) => {
        config_text = JSON.stringify(value, null, 2)
    })

    onMount(async () => {
        
	});
    
    
</script>

<style>
    .code-block {
        /* display: block; */
    }
</style>

<button class="page-button" on:click={goHome}><span class="material-icons">arrow_circle_left</span></button>
<h1>Configuration</h1>
<pre class="code-block"><code>{config_text}</code></pre>
<pre class="code-block"><code>{config_text}</code></pre>

