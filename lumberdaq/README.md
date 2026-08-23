# Lumberdaq
Rust library for data aquisition

## Install
### Windows
...

## Development
```cmd
cargo run
```

# Todo
- Results files named by start date and time. Example scripts should delete existing results files to keep things clean.
- Review SQLite vs csv for data storage. Becbnark speeds.
- Clean up repo, moving and tauri files into separate folder.
- Add threads for each device.
- Add pico technology data loggers. This crate for the oscilloscopes may have reusable patterns including runtime driver download but doesn't cover data loggers [pico_sdk](https://docs.rs/pico-sdk/latest/pico_sdk/).
- Create installer including any required .dll files.