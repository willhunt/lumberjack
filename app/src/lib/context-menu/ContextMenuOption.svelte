<script lang="ts">
	import { onMount, getContext } from 'svelte';
	import { key } from './context-menu.js';
	
	export let isDisabled = false;
	export let text = '';
	
	import { createEventDispatcher } from 'svelte';
	const dispatch = createEventDispatcher();	
	
	const { dispatchClick } = getContext(key) as any;
	
	const handleClick = (event: MouseEvent) => {
		if (isDisabled) return;
		
		dispatch('click');
		dispatchClick();
	}
</script>

<style>
	.context-menu-button {
		/* padding: 4px 4px; */
		cursor: default;
		font-size: 12px;
		display: flex;
		align-items: left;
		border-radius: 5px;
		margin: 2px;
		/* grid-gap: 5px; */
	}
	.context-menu-button:hover {
		background: #0002;
	}
	.context-menu-button.disabled {
		color: rgba(105, 105, 105, 0.4);
	}
	.context-menu-button.disabled:hover {
		background: white;
	}
</style>

<button class="{isDisabled ? 'disabled': ''} context-menu-button hidden-button" on:click={handleClick}>
	{#if text}
		{text}
	{:else}
		<slot />
	{/if}
</button>
