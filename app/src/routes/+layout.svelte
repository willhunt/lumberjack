<script lang="ts">
    import { onMount } from 'svelte'
    import 'material-icons/iconfont/filled.css';
    // import 'material-icons/iconfont/material-icons.css';
    // import 'material-icons/iconfont/outlined.css';
    import { config_store, nav_width_store } from '$lib/stores';
    import { goto } from '$app/navigation'
    import { loadAppConfig } from '$lib/tools'
    import { resourceDir, resolve } from '@tauri-apps/api/path'
    import type { ConfigStore, Device, Dashboard } from '$lib/types'
    import ExplorerItemDashboard from '$lib/explorer-item-dashboard/ExplorerItemDashboard.svelte';
    let explorer_menu

    // let devices: Device[] = []
    // let dashboards: Dashboard[] = []

    let explorer_display = 'inline-block'
    function hideExplorer() {
        explorer_display = explorer_display == 'inline-block' ? 'none' : 'inline-block'
    }
    function showConfig() {
        goto("/configuration")
    }

    function addDevice() {
        // Creare new device
        let name = "New device"
        let short_name = name.replace(/\s+/g, '-').toLowerCase();  // lowercase with dashes
        $config_store.devices[short_name] = {name: name,  plugin: "", "channels": []}
        goto("/device/" + short_name)
    }
    function openDevice (name: string) {
        goto("/device/" +  name)
    }

    function addDashboard() {
         // Creare new device
        let name = "New dashboard"
        // let short_name = name.replace(/\s+/g, '-').toLowerCase();  // lowercase with dashes
        $config_store.dashboards.push({name: name, widgets: []})
        $config_store = $config_store  // Need this to update view
        goto("/edit_dashboard/" + ($config_store.dashboards.length - 1).toString())
    }


    let nav_resize_active = false
    function startResizeNav(event: MouseEvent) {
        nav_resize_active = true
    }
    function resizeNav(event: MouseEvent) {
        if (nav_resize_active) {
            event.preventDefault()
            let new_width = $nav_width_store + event.movementX
            if (new_width > 200 && new_width < 500) {
                nav_width_store.set(new_width)
            }
        }
    }
    function stopResizeNav(event: MouseEvent) {
        nav_resize_active = false
    }

    onMount(async () => {
        const resources_path = await resourceDir();
        const mock_path = await resolve(resources_path, 'examples', 'mock_config.json')
        loadAppConfig(mock_path)
        // config_store.subscribe((value: ConfigStore) => {
        //     devices = value.devices
        //     dashboards = value.dashboards
        // })
	});
</script>

<style>
    .vertnav {
        height: 100%;
        width: 60px;
        background-color: #333333;
        text-align: center;
        position: fixed;
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
        width: calc(var(--nav-width) * 1px);
        background-color: #252526;
        display: var(--explorer_display);
        position: fixed;
        left: 60px;
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
    :global(.explorer-section-item) {
        padding-left: 22px;
        padding-right: 22px;
        padding-top: 5px;
        padding-bottom: 5px;
        color: #c4cccc;
        display: flex;
        font-size: 12px;
        vertical-align: middle;
    }
    :global(.active-icon) {
        display: flex;
        vertical-align: middle;
        font-size: 16px;
    }
    .section-icon {
        font-size: 16px;
        color: #c4cccc;
    }
    :global(.flexbox-push) {
        margin-left: auto;
    }
    :global(.hidden-button) {
        background-color: inherit;
        border: none;
        color: inherit;
        font-size: inherit;
        cursor: pointer;
    }
    .resize-handle {
        position: fixed;
        height: 100%;
        width: 6px;
        left: calc(var(--nav-width) * 1px + 60px);
        cursor: col-resize;
    }
    .resize-handle:hover {
        background-color: #c4cccc;
    }
    
    .viewer {
        margin-left: calc(var(--nav-width) * 1px + 60px);
        font-size: 14px;
        padding: 20px;
    }
    
</style>
<!-- <ExplorerContextMenu bind:this={explorer_menu}  /> -->
<div class="layout" on:mousemove="{resizeNav}" on:mouseup="{stopResizeNav}" on:mouseleave="{stopResizeNav}" style=" --nav-width:{$nav_width_store}; height:100%">
    <div class="sidebar" >
        <div class="vertnav">
            <button class="explorer-button" on:click={hideExplorer}><span class="vertnav-icon material-icons">input</span></button>
            <button class="explorer-button" on:click={showConfig}><span class="vertnav-icon material-icons">settings</span></button>
        </div>
        <div class="explorer" style="--explorer_display: {explorer_display};">
            <div class="explorer-title">EXPLORER</div>
            <div class="explorer-section-header">
                <div>Devices</div>
                <div class="flexbox-push">
                    <button class="explorer-button" on:click={addDevice}><span class="section-icon material-icons">add_circle</span></button>
                </div>
            </div>
            {#each Object.entries($config_store.devices) as [devicename, device]}
                <div class="explorer-section-item">
                    <div><span class="material-icons  active-icon">chevron_right</span></div>
                    <button class="hidden-button" on:click={() => openDevice(devicename)}>{device.name}</button>
                    <div class="flexbox-push"><span class="material-icons active-icon">noise_control_off</span></div>
                </div>
            {/each}
            <div class="explorer-section-header">
                <div>Dashboards</div>
                <div class="flexbox-push">
                    <button class="explorer-button" on:click={addDashboard}><span class="section-icon material-icons">add_circle</span></button>
                </div>
            </div>
            {#each $config_store.dashboards as dashboard, i}
                <ExplorerItemDashboard dashboard_index={i} dashboard_name={dashboard.name}/>
                <!-- <div class="explorer-section-item">
                    <div><span class="material-icons active-icon">chevron_right</span></div>
                    <button class="hidden-button" on:click={() => openDashboard(i)} on:contextmenu|preventDefault={explorer_menu.onRightClick}>{dashboard.name}</button>
                    <div class="flexbox-push"><span class="material-icons active-icon">noise_control_off</span></div>
                </div> -->
            {/each}
        </div>
        <div class="resize-handle" on:mousedown="{startResizeNav}"></div>
    </div>
    <div class="viewer">
        <slot />
    </div>
</div>
