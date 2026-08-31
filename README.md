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
National Instruments is supported as a legacy option, because we still own
USB-6001 hardware and still have to read it. It is not the direction.

## Drivers

**No vendor software is needed to build this, or to run it against hardware you
do not own.** Bindings are generated once and committed rather than generated
during the build, and each driver is looked up at run time with `libloading`
rather than linked. So the workspace compiles on a machine with none of it
installed, and a driver that is not there fails when a device is opened, saying
what it looked for.

| you are | to build | to read that hardware | to regenerate bindings |
|---|---|---|---|
| using no DAQ hardware | nothing | — | — |
| reading a Pico ADC-20/24 | nothing | PicoSDK runtime (`picohrdl.dll`) | PicoSDK headers, LLVM |
| reading an NI USB-6001 | nothing | NI-DAQmx runtime (`nicaiu.dll`) | NI-DAQmx Support for C, LLVM |

Only whoever regenerates bindings needs a header and a working bindgen, and that
is done once per driver, not once per build.

### Installing NI-DAQmx

*NI support is being added; this is what it will want.*

The installer offers a long list of extras. Two matter:

| tick | why |
|---|---|
| **NI-DAQmx Runtime with Configuration Support** | the driver itself, plus MAX for seeing and naming devices |
| **NI-DAQmx Support for C** | `NIDAQmx.h`, and only for regenerating bindings |

Untick the rest. **.NET Framework 4.0 and 4.5 support** are for a language we do
not use; **NI Linux RT System Image**, **Web-Based Configuration and
Monitoring** and **cDAQ Firmware** are for hardware a USB-6001 is not. **NI
Certificates** only suppresses Windows security prompts during installation, and
**NI Hardware Manager** duplicates what MAX already does. **NI I/O Trace** logs
every DAQmx call and is worth having only while working on the bindings.

If you are only *running* lumberdaq against a 6001, the runtime alone is enough:
Support for C is a build-time convenience for this repository, not a dependency
of the program.

DAQmx addresses channels by device name, as in `Dev1/ai0`, so a device needs a
name before it can be read. MAX assigns one when the hardware is plugged in, and
will also create a **simulated** device, which is enough to develop against with
nothing attached.


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
