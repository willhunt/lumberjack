import { resourceDir, resolve } from '@tauri-apps/api/path'
import { readTextFile, BaseDirectory } from '@tauri-apps/api/fs'
import { config_store } from '$lib/stores'

export async function loadAppConfig(config_path: string) {
    // console.log(config_path)
    const file_contents = await readTextFile(config_path)
    let json_config = JSON.parse(file_contents)
    console.log(json_config)
    config_store.set(json_config)
}
