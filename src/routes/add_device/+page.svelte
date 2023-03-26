<script>
    import SerialPorts from '../../lib/SerialPorts.svelte'

    function goHome() {
        location.href="/"
    }
</script>

<style>
    .page-button {
        padding: 0px;
        background-color: inherit;
        border: none;
        cursor: pointer;
    }
    .page-button span {
        font-size: 30px;
    }
</style>

<button class="page-button" on:click={goHome}><span class="material-icons">arrow_circle_left</span></button>
<h1>Add device</h1>
<SerialPorts />