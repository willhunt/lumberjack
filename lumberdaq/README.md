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

Each device is read on its own thread, so a slow device does not hold up a fast
one.

### Rates

Two different things get called a rate, and only one of them is about the data.

**`read_interval_ms`**, on the device, is how often lumberdaq collects from it.
It defaults to 100 ms and usually needs no thought. It is *not* how often the
data is saved: that happens on its own one second timer regardless.

**Sample rate** is how fast the hardware measures. It only appears as a setting
where we can actually command the instrument:

| device | who sets the sample rate | setting |
|---|---|---|
| Pico, polled | us, implicitly: a read *is* a sample | `read_interval_ms` |
| Pico, streaming | us, explicitly: the unit is told to scan at this rate | `acquisition.sample_interval_ms` |
| Mock, streaming | us, for a simulated device | `acquisition.sample_interval_ms` |
| Serial | **the device. There is no setting, because we cannot change it** | none |

So for a polled Pico, `read_interval_ms` really is your sample rate. For anything
that samples on its own schedule it is only a collection rate: it changes how
promptly data reaches disk, and nothing about the data. Set it slower on a serial
device and you get exactly the same samples with exactly the same timestamps, in
larger batches, later.

That is why timestamps are trustworthy whatever it is set to. They come from the
instrument for a streaming Pico, from the schedule for a streaming mock, and from
when the bytes arrived for serial - never from when we happened to collect.

### What happens when

```
device  --read_interval_ms-->  batches  --channel-->  sink  --1 s-->  disk
        per device, default 100ms       immediately          fixed
```

## Test projects

Four setups under `test_projects/`, each showing one thing:

| | needs hardware | shows |
|---|---|---|
| `mock_sine` | no | streaming with nothing plugged in. Two sine channels sampled at 100 Hz, collected far more slowly. The values can be checked against the wave they claim to be. |
| `simulated_and_serial_devices` | a serial device on COM3 | a mock and a real device in one run, at different rates, on their own threads |
| `pico_adc20` | ADC-20 or ADC-24 | polled acquisition, where `read_interval_ms` is the sample rate |
| `pico_adc20_stream` | ADC-20 or ADC-24 | the same unit told to scan itself, so samples carry the unit's own timestamps |

`mock_sine` is the one to run first, since it needs nothing attached:

```cmd
cargo run -- test_projects/mock_sine
```

### Differential inputs on a Pico logger

A channel measures against ground unless it says otherwise:

```json
{ "name": "Delta P", "unit": "V", "description": "",
  "channel": 1, "range": "milli_volts2500", "single_ended": false }
```

A differential input measures between a channel and the one above it, so the
first of the pair is always odd and the even channel beside it is consumed. An
ADC-20 has eight inputs and therefore four differential pairs, 1-2, 3-4, 5-6 and
7-8; an ADC-24 has sixteen and eight pairs.

Configuring that wrongly is refused before a run rather than at connect, naming
the pair you probably meant. Whether a channel exists at all depends on the
model, so that is checked when the unit is opened and can say which it is.

## Calculated channels

A sensor reports volts when the quantity you want is pressure. A calculated
channel applies an equation to measured channels and records the result beside
them, under a device of its own so what was measured stays distinct from what
was worked out.

```json
"calculated": {
  "info": { "name": "Derived", "description": "" },
  "channels": [
    { "name": "Delta P", "unit": "bar", "description": "",
      "inputs": { "high": { "device": "Transducer", "channel": "High" },
                  "low":  { "device": "Transducer", "channel": "Low" } },
      "equation": "high - low" }
  ]
}
```

Inputs are given short names because channel names have spaces and quoting those
inside an expression is miserable. The usual arithmetic works, along with
`sqrt`, `abs`, `ln`, `log10`, `exp`, the trigonometric functions and `round`.

Equations are read at run time, so a program embedding lumberdaq can let someone
write one while it is running. `CalculatedChannel::validate` runs exactly the
checks a run would, and `DaqConfig::available_inputs` lists what can be referred
to, which is what a text box and a dropdown need.

### Combining channels that sample at different times

Two devices sampling at the same rate still sample at different moments, so
values have to be paired rather than matched. A calculated channel is driven by
its **slowest** input, and every other input contributes its nearest sample
within half of that input's own period. Nothing to configure: the periods come
from the setup, or are measured for a device that streams at a rate of its own.

Being driven by the slowest input is what keeps it accurate. Measured on a 1 Hz
channel against a 10 Hz one:

```text
trigger on the slow input, pair with the fast:   median   1.8 ms
trigger on the fast input, pair with the slow:   median 290.7 ms
```

Two consequences worth knowing. The output appears at the **slowest** input's
rate; producing it faster would mean repeating a value that was never measured.
And if an input has no sample near a trigger, because its device stopped, that
sample is skipped and reported rather than paired with something stale.

Channels of a single device share timestamps exactly, so a differential between
two channels of one transducer is exact rather than paired.

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
