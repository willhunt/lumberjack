<script>
    import { invoke } from '@tauri-apps/api/tauri'
  
    let serial_ports = ['None detected']
  
    async function list_serial_ports() {
        serial_ports = await invoke('list_serial_ports')
    }
</script>

<div>
    <button on:click="{list_serial_ports}">List Serial Ports</button>
    {#each serial_ports as serial_port}
        <li>
            {serial_port}
        </li>
    {/each}
</div>