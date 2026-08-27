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

### Checking a setup without running it

```cmd
cargo run -- check test_projects/scaled
```

Builds everything a recording would build and reports what is wrong, without
connecting to any hardware or writing a results file. Useful away from the rig,
and before a run that matters.

Problems are collected a part at a time rather than stopping at the first, so
three misconfigured devices take one run of this to find rather than three:

```
    ok    device 'Good rig', 1 channel(s)
    FAIL  device 'Bad scale'
          the scale for 'Flow' uses 'v', which it has no value for. Available: x (the measurement). Scale: v * 2
    FAIL  device 'Bad pico'
          channel 2 cannot be differential: a differential input pairs a channel with the one above it,
          so the first of the pair must be odd. Use channel 1 to measure between 1 and 2
    FAIL  calculated channel 'Missing source'
          'Missing source' reads Ghost/Nothing, which no device provides
```

It exits non-zero when anything failed, so it is worth something in a script.
`Project::check` is the same thing for a program embedding the library, and
`check_config` takes a configuration that has not been saved yet.

What it cannot tell you is whether the hardware answers. That needs the rig.

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

### Recording to more than one place

A run has one sink, so recording to two places is a sink that is itself two:

```rust
daq.set_sink(Box::new(
    Fanout::new()
        .and("sqlite", project.sink(StorageFormat::Sqlite)?)
        .and("csv", project.sink(StorageFormat::Csv)?),
))?;
```

`Fanout` is a `DataSink` like any other, so nothing in `Daq` knows the
difference. Every sink is offered each batch even if an earlier one fails, so a
sink that has gone wrong cannot starve the others; the failure is reported
afterwards and names all of them, since a full disk fails every sink at once.

This is also how a live display attaches: it is a sink alongside the file it is
being written to, not a second path through the library.

```cmd
cargo run --example two_sinks -- test_projects/scaled
```

### Starting and stopping recording

A run holds its sink for the whole run, so nothing can hand one over partway
through when somebody presses record. A `Recorder` is attached from the start
instead and writes only while a flag is set:

```rust
let recording = Arc::new(AtomicBool::new(false));
daq.set_sink(Box::new(Recorder::new(
    Arc::clone(&recording),
    Box::new(move || project.sink_for(storage, &label())),
)))?;
```

The sink underneath is built when recording starts rather than up front, so a
session that is only being watched leaves no results file behind at all. It is
built afresh each time, so stopping and starting gives a second recording rather
than more of the first. `Project::sink_for` decides what that means per format:
a database keeps every run in the one file and its runs table tells them apart,
while a CSV gets a file of its own each time, since nothing in a CSV can say
where one recording ends and the next begins.

Devices keep reading throughout. What is not recorded is still acquired, which
is what lets a display show live readings before anyone has pressed anything.

```cmd
cargo run --example record_in_bursts -- test_projects/scaled
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

Setups under `test_projects/`, each showing one thing:

| | needs hardware | shows |
|---|---|---|
| `scaled` | no | scaled channels, recording the sensor's units rather than volts, with and without named constants |
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

## Scaling a channel

A sensor reports volts when the quantity you want is litres per minute. A scale
converts each reading as it arrives, so the channel records the quantity and not
the voltage it was measured as. `x` is the raw measurement:

```json
{ "name": "Pressure", "unit": "bar", "description": "0-10 bar transducer",
  "scale": "x * 5 + 5" }
```

Any channel on any device can take a `scale`; leave it out and readings are
stored as they came.

### Keeping the numbers editable

Written that way, the constants dissolve into the arithmetic. `x / 120` gives no
hint that 120 is a shunt resistor, so refitting a 100 ohm one means working the
equation out again, and no saved project can say what sensor it was for.

Naming them instead keeps them editable:

```json
{ "name": "Flow", "unit": "L/min", "description": "0-29 L/min flow meter",
  "scale": {
    "from": "4-20 mA transmitter",
    "equation": "(((x / shunt_ohms) * 1000 - 4) / 16) * (high - low) + low",
    "parameters": { "shunt_ohms": 120, "low": 0, "high": 29 }
  } }
```

Both forms are the same equation at run time — the constants are simply bound
alongside `x`. `from` is a label and nothing more, naming the sensor definition
the numbers came from so an interface can offer the right form for editing them.
The equation is copied in rather than referred to, so a project still runs when
that definition is not to hand.

`Scale::parameters` and `Scale::from` are what a form reads to fill itself in.

### What is checked, and when

A scale that will not parse, or that reads a name it has no value for, is
refused when the project is loaded rather than on the first reading of a run.
The message lists what was available, since the cause is almost always a typo:

```
the scale for 'Flow' uses 'shunt', which it has no value for.
Available: x (the measurement), high, low, shunt_ohms. Scale: x / shunt
```

A reading that cannot be scaled at all is left out and reported; the others from
the same read are still kept.

**The raw reading is not kept.** That is the point of it, since nobody wants
volts from a flow meter, but it does mean the scale is the only way back to the
measurement. It is written into the results with the rest of the config, so a
wrongly scaled run can be recovered as long as the equation can be undone —
which for a multiplication or an offset it always can.

`test_projects/scaled` shows both forms with an unscaled channel beside them for
comparison. It needs no hardware.

## Calculated channels

A scale reads one channel. When a value needs several — a differential pressure
from two transducers — a calculated channel applies an equation across measured
channels and records the result beside them, under a device of its own so what
was measured stays distinct from what was worked out.

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

This crate is a member of the workspace at the repository root, so build output
goes to `../target` and `cargo test --workspace` from there covers picolog and
choptui as well.

# Todo
- Add pico technology TC-08.
- Post processing.
- Create installer including any required .dll files.
