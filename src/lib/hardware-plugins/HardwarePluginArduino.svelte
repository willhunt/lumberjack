<script>
    import { invoke } from '@tauri-apps/api/tauri'
    import { onMount } from 'svelte';
  
    let serial_ports = [{name: "None", product: "Unknown", port_type: "Unknown"},]
    let baudrates = [300, 600, 1200, 2400, 4800, 9600, 14400, 19200, 28800, 31250, 38400, 57600, 115200]
  
    async function list_serial_ports() {
        serial_ports = await invoke('list_serial_ports')
    }

    onMount(async () => {
		list_serial_ports()
	});
    
</script>

<style>

</style>

<div>
<form>
    <div class="form-line">
        <label>
            Serial Port:
            <select name="port">
                {#each serial_ports as serial_port}
                    <option value="{serial_port.name}">
                        {serial_port.name} | {serial_port.product} | {serial_port.port_type}
                    </option>
                {/each}
            </select>
        </label>
        <button class="inline-button" on:click="{list_serial_ports}"><span class="inline-icon material-icons">refresh</span></button>
    </div>
    <div class="form-line">
        <label>
            Baudrate:
            <select name="baudrate">
                {#each baudrates as baudrate}
                    <option value="{baudrate}" selected={baudrate===115200}>
                        {baudrate}
                    </option>
                {/each}
            </select>
        </label>
    </div>
    <button class="btn-main">Add device</button>
    
</form>
</div>