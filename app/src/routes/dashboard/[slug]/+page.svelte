<script lang="ts">
    import { onMount } from 'svelte'
    import { invoke } from '@tauri-apps/api/tauri'
    import { resourceDir } from '@tauri-apps/api/path';
    import type { ConfigStore, Widget } from '$lib/types'
    import { config_store } from '$lib/stores';
	import { afterNavigate } from '$app/navigation';

    // import {WidgetNumeric} from '$lib/widgets/WidgetNumeric.svelte';
    // let w = WidgetNumeric
    export let data
    // let dashboard_widgets: Widget[] = []
    $: dashboard_widgets = $config_store.dashboards[Number(data.slug)].widgets
    let component_hash: { [key: string]: any } = {}

    async function loadLibraryWidgets() {
        // construct list of dahsboard widget name
        let dashboard_widget_names = dashboard_widgets.map(({name}) => name)
        let library_widget_names: String[] = []
        // let widget_shortnames = []
        // https://vitejs.dev/guide/features.html#glob-import
        const file_imports = import.meta.glob("$lib/widgets/Widget*.svelte")
        for (const path in file_imports) {
            let file_name = path.split("/").pop()
            if (file_name) {  // Required as file_name may be undefined, (typescript)
                let component_name = file_name.split(".")[0]
                library_widget_names.push(component_name)
                let widget_name = component_name.slice(6).toLowerCase()  // remove "Widget" prefix and make lower
                if (dashboard_widget_names.includes(widget_name)) {
                    // Import plugins dynamically
                    // https://svelte.dev/repl/16b375da9b39417dae837b5006799cb4?version=3.25.0
                    // let component = (await import(path)).default
                    // component_hash[widget_name] = component
                    file_imports[path]().then((module: any) => {
                        component_hash[widget_name] = module.default
                    })
                }
            }
        }
        return library_widget_names
    }
    afterNavigate(() => {
        console.log("Dashboard loaded: "+ data.slug)
        if (dashboard_widgets.length > 0) {
            loadLibraryWidgets()
        }

    })

    
</script>

<style>
    
</style>

<div class="dashboard">
    {#each dashboard_widgets as widget}
        <svelte:component this={component_hash[widget.name]}  channel={widget.channels[0]}/> <!-- objAttributes={} -->
    {/each}
</div>
