# Lumberdaq
Rust library for data aquisition

## Install
### Windows
...

## Running

A *project* is a directory holding `config.json`, which describes the devices and
channels, and the results recorded from them. Nothing about a rig is compiled in,
so changing a channel means editing that file, not rebuilding.

Run one by pointing at its directory:

```cmd
cargo run -- test_projects/simulated_and_serial_devices
```

Everything after `--` goes to the program rather than to cargo. With no argument
it uses the current directory, so from inside a project:

```cmd
cd test_projects/simulated_and_serial_devices
cargo run
```

Recording continues until **Ctrl-C**, which stops the run tidily and flushes
whatever is still buffered. Killing the process instead loses that.

```cmd
cargo run -- --help
```

## Creating a project

`build_config` defines a rig in Rust and writes it out as a config. That is a
convenience for producing something to test against, not how a setup is normally
made.

```cmd
cargo run --example build_config -- test_projects/my_project
```

It overwrites `config.json` in that directory, so edit the example rather than
the config if you are iterating on a rig.

## Using it from another program

```rust
let mut daq = lumberdaq::open("my_project")?;   // reads config, attaches the sink

let report = daq.connect();                      // which devices came up
daq.run(&stop, &mut on_event)?;                  // records until `stop` is set
```

`connect` and `run` are separate so a program can decide for itself what a partly
connected rig means, and can show what happens while a run is going. `run` blocks
for the length of the run and reports through the callback, because each device
is being read by its own thread and nothing else can see it until they finish.

A worked example, in the shape a TUI or GUI would use:

```cmd
cargo run --example embed -- test_projects/simulated_and_serial_devices
```

## Project directory

```
my_project/
    config.json      the setup: devices, channels, sample rates, storage format
    results.db       sqlite results, if storage is "sqlite"
    results.csv      csv results, if storage is "csv"
    results.json     the sidecar describing results.csv
```

Results files are gitignored; `config.json` is not.

`config.json` names its own storage format, so a project cannot be run one way
today and another tomorrow and end up with half its data in each.

| `"storage"` | |
|---|---|
| `"sqlite"` | Default. One file holding the setup and every run. Readable while recording, so a plot can watch a run in progress. Recording again appends a run rather than overwriting. |
| `"csv"` | Long format, plus a json sidecar. Readable by anything, and a run that dies halfway still leaves a usable file. Recording again **overwrites**. |

Each device has its own `sample_interval_ms` and is read on its own thread, so a
slow device does not hold up a fast one.

## Development

```cmd
cargo test
cargo check --all-targets
```

`cargo check` on its own does not build examples, so run it with
`--all-targets` or `cargo test` to catch breakage there.

# Todo
- Add pico technology data loggers. This crate for the oscilloscopes may have reusable patterns including runtime driver download but doesn't cover data loggers [pico_sdk](https://docs.rs/pico-sdk/latest/pico_sdk/).
- Create installer including any required .dll files.
