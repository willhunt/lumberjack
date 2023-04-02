<script lang="ts">
    import { onMount } from 'svelte'
    import 'material-icons/iconfont/filled.css';
    // import 'material-icons/iconfont/material-icons.css';
    // import 'material-icons/iconfont/outlined.css';
    import { config_store } from '$lib/stores';
    import { loadAppConfig } from '$lib/tools'
    import { resourceDir, resolve } from '@tauri-apps/api/path'
    import type { ConfigStore, Device, Dashboard } from '$lib/types'

    let devices: Device[] = []
    let dashboards: Dashboard[] = []

    

    let explorer_display = 'inline-block'
    function hideExplorer() {
        explorer_display = explorer_display == 'inline-block' ? 'none' : 'inline-block'
    }
    function showConfig() {
        location.href="configuration"
    }

    function addDevice() {
        location.href="add_device"
    }
    function addLayout() {
        location.href="dashboard"
    }

    onMount(async () => {
        const resources_path = await resourceDir();
        const mock_path = await resolve(resources_path, 'examples', 'mock_config.json')
        loadAppConfig(mock_path)
        config_store.subscribe((value: ConfigStore) => {
            devices = value.devices
            dashboards = value.dashboards
        })
	});
</script>

<style>
    .vertnav {
        height: 100%;
        width: 60px;
        /* position: fixed;  Fixed Sidebar (stay in place on scroll) */
        background-color: #333333;
        text-align: center;
        display: inline-block;
        font-size: 0px;
        vertical-align: top;
    }

    .explorer-button {
        padding: 0px;
        display: block;
        margin: auto;
        background-color: inherit;
        border: none;
        width: 100%;
        cursor: pointer;
    }
    .vertnav-icon {
        padding-top: 14px;
        padding-bottom: 14px;
        margin: auto;
        font-size: 28px;
        color: #c4cccc;
    }
   
    .explorer {
        height: 100%;
        width: 300px;
        background-color: #252526;
        display: var(--explorer_display);
        font-size: 14px;
        vertical-align: top;
    }
    .explorer-title {
        padding: 20px;
        color: #c4cccc;
        font-size: 16px;
    }
    .explorer-section-header {
        padding-left: 22px;
        padding-right: 22px;
        padding-top: 5px;
        padding-bottom: 5px;
        background-color: #2d2d2e;
        color: #c4cccc;
        display: flex;
        vertical-align: middle;
    }
    .explorer-section-item {
        padding-left: 22px;
        padding-right: 22px;
        padding-top: 5px;
        padding-bottom: 5px;
        color: #c4cccc;
        display: flex;
        font-size: 12px;
        vertical-align: middle;
    }
    .active-icon {
        display: flex;
        vertical-align: middle;
        font-size: 16px;
    }
    .section-icon {
        font-size: 16px;
        color: #c4cccc;
    }
    .flexbox-push {
        margin-left: auto;
    }
    
    .viewer {
        display: inline-block;
        font-size: 14px;
        padding: 20px;
    }
    
</style>
<div class="vertnav">
    <button class="explorer-button" on:click={hideExplorer}><span class="vertnav-icon material-icons">input</span></button>
    <button class="explorer-button" on:click={showConfig}><span class="vertnav-icon material-icons">settings</span></button>
    
    <!-- <object class="vertnav-button" data="/input_FILL0_wght400_GRAD0_opsz48.svg" style="color: white;" title="input"/> -->
    <!-- <img class="vertnav-button" src="/input_FILL0_wght400_GRAD0_opsz48.svg" alt="Input"/> -->
</div>
<div class="explorer" style="--explorer_display: {explorer_display};">
    <div class="explorer-title">EXPLORER</div>
    <div class="explorer-section-header">
        <div>Devices</div>
        <div class="flexbox-push">
            <button class="explorer-button" on:click={addDevice}><span class="section-icon material-icons">add_circle</span></button>
        </div>
    </div>
    {#each devices as device}
        <div class="explorer-section-item">
            <div><span class="material-icons  active-icon">chevron_right</span></div>
            <div>{device.name}</div>
            <div class="flexbox-push"><span class="material-icons active-icon">noise_control_off</span></div>
        </div>
    {/each}
    <div class="explorer-section-header">
        <div>Layouts</div>
        <div class="flexbox-push">
            <button class="explorer-button" on:click={addLayout}><span class="section-icon material-icons">add_circle</span></button>
        </div>
    </div>
    {#each dashboards as dashboard}
        <div class="explorer-section-item">
            <div><span class="material-icons  active-icon">chevron_right</span></div>
            <div>{dashboard.name}</div>
            <div class="flexbox-push"><span class="material-icons active-icon">noise_control_off</span></div>
        </div>
    {/each}
</div>
<!-- <div class="resize-handle--x" data-target="aside"></div> -->
<div class="viewer">
    <slot />
</div>
    