# Lumberjack
Work in progress. Do not use.

Data aquisition software.

# Layout
* `lumberdaq/` — the data acquisition library and its CLI. The active work.
* `app/` — an earlier Tauri + Svelte prototype. Does not use `lumberdaq` yet.
* `sandbox/` — throwaway experiments, kept out of the library's dependencies.

# Hardware
Pico Technology and other open DAQ hardware, plus devices streaming over serial.

# Run Development
```ps
cd .\lumberdaq\
cargo run
```

The Tauri prototype:

```ps
cd .\app\src-tauri\
cargo tauri dev
```
