<script lang="ts">
    import { config_store, nav_width_store } from '$lib/stores';
    import { goto } from '$app/navigation'
    import ContectMenu from '../context-menu/ContextMenu.svelte'
	import ContectMenuOption from '../context-menu/ContextMenuOption.svelte'
	import ContectMenuDivider from '../context-menu/ContextMenuDivider.svelte'

    export let dashboard_index: number = 0
    export let dashboard_name: string = ""
    let showMenu = false
    let pos = { x: 0, y: 0 }

    function openDashboard (index: number) {
        goto("/dashboard/" +  index.toString())
    }

    export async function onRightClick(event: MouseEvent) {
		if (showMenu) {
			showMenu = false
			await new Promise(res => setTimeout(res, 100))
		}
		
		pos = { x: event.clientX - 60, y: event.clientY };  // Offset x by fixed left nav bar wdth
		showMenu = true
		console.log("pos:", pos)
	}
	
	function closeMenu() {
		showMenu = false
	}

</script>

<style>

</style>

<div class="explorer-section-item">
    <div><span class="material-icons active-icon">chevron_right</span></div>
    <button class="hidden-button" on:click={() => openDashboard(dashboard_index)} on:contextmenu|preventDefault={onRightClick}>{dashboard_name}</button>
    <div class="flexbox-push"><span class="material-icons active-icon">noise_control_off</span></div>
</div>

{#if showMenu}
	<ContectMenu {...pos} on:click={closeMenu} on:clickoutside={closeMenu}>
		<ContectMenuOption 
			on:click={() => goto("/edit_dashboard/" + dashboard_index.toString())} 
			text="Edit" />
		<!-- <ContectMenuDivider /> -->
		<ContectMenuOption 
			isDisabled={true} 
			on:click={console.log} 
			text="Delete" />
	</ContectMenu>
{/if}