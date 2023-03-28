<script>
    import 'material-icons/iconfont/filled.css';
    // import 'material-icons/iconfont/material-icons.css';
    // import 'material-icons/iconfont/outlined.css';

    let explorer_display = 'inline-block'
    function hideExplorer() {
        explorer_display = explorer_display == 'inline-block' ? 'none' : 'inline-block'
    }

    function addDevice() {
        location.href="add_device"
    }
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
</div>
<!-- <div class="resize-handle--x" data-target="aside"></div> -->
<div class="viewer">
    <slot />
    <!-- Hello, this is my app -->
</div>
    