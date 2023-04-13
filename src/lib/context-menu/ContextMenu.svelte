<script lang="ts">
	import { onMount, setContext, createEventDispatcher } from 'svelte'
	import { fade } from 'svelte/transition'
	import { key } from './context-menu.js'

	export let x: number
	export let y: number
    let menuEl: any  // Menu element
	
	// whenever x and y is changed, restrict box to be within bounds
	$: (() => {
		if (!menuEl) return;
		
		const rect = menuEl.getBoundingClientRect()
		x = Math.min(window.innerWidth - rect.width, x)
		if (y > window.innerHeight - rect.height) y -= rect.height
	})
	
	const dispatch = createEventDispatcher();	
	
	setContext(key, {
		dispatchClick: () => dispatch('click')
	})
	
	
	function onPageClick(event: MouseEvent) {
		if (event.target === menuEl || menuEl.contains(event.target)) return
		dispatch('clickoutside')
	}
</script>

<style>
	div {
		position: absolute;
		display: grid;
		border: 1px solid #0003;
		border-radius: 5px;
		box-shadow: 2px 2px 5px 0px #0002;
		background: white;
		z-index: 9999;
	}
</style>

<svelte:body on:click={onPageClick} />

<div transition:fade={{ duration: 100 }} bind:this={menuEl} style="top: {y}px; left: {x}px;">
	<slot />
</div>