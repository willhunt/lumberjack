# Lumberjack
Work in progress. Do not use.

Data aquisition software.

# Layout
A Cargo workspace over the Rust crates, so they share one `target/` directory
and one `Cargo.lock`.

* `lumberdaq/` — the data acquisition library and its CLI. The active work.
* `picolog/` — safe wrappers over the Pico Technology driver, used by lumberdaq.
* `choptui/` — terminal monitor for a run. A separate crate on purpose: it can
  only reach lumberdaq's public API, so a missing export fails to compile here.
* `app/` — an earlier Tauri + Svelte prototype. Does not use `lumberdaq` yet.
* `sandbox/` — throwaway experiments, kept out of the library's dependencies.

The last two stand outside the workspace and keep their own dependencies.

# Hardware
Pico Technology and other open DAQ hardware, plus devices streaming over serial.

# Run Development
```ps
cd .\lumberdaq\
cargo run
```

Or from the repository root, for any one crate:

```ps
cargo run -p lumberdaq -- lumberdaq/test_projects/scaled
cargo test --workspace
```

The Tauri prototype:

```ps
cd .\app\src-tauri\
cargo tauri dev
```
