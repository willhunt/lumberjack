# choptui

A terminal monitor for a lumberdaq acquisition.

From the repository root:

```cmd
cargo run -p choptui -- lumberdaq/test_projects/scaled
```

Or from inside `lumberdaq/`, which is where the CLI is usually run:

```cmd
cargo run -p choptui -- test_projects/scaled
```

The path is relative to wherever you are, not to the crate, so `-p choptui`
does not change what a relative path means. It reads the same project directory
the CLI records from.

```
LUMBERJACK  test_projects/scaled                             not recording
Devices  -  Plots  -  Log  -  Settings

┌ Rig ──────────────────────── Connected ┐
│                                        │
│ Flow         14.500 L/min      63      │
│ Pressure     7.250 bar         61      │
│                                        │
└────────────────────────────────────────┘

┌ Missing rig ─────────── port not found ┐
│                                        │
│ Temperature  --- C             0       │
│                                        │
└────────────────────────────────────────┘
```

`Tab` and the arrow keys move between tabs. `q`, `Esc` or `Ctrl-C` stops the
run and quits.

## What it does so far

Devices only. Every channel in the setup, its latest reading and how many have
arrived, with each device's connection status and why it is not connected if it
is not. The Log tab shows what the devices have reported. Plots and Settings
are not built yet and say so.

**Nothing is recorded.** It connects, reads and displays. Deliberately not
`lumberdaq::open`, which attaches the project's sink and so creates the results
file before anything has been recorded — watching a rig should leave nothing
behind. The record button in the header is the next piece.

## How it is put together

A separate crate rather than part of lumberdaq, so it can only reach the public
API. Anything it needs that is not exported fails to compile here rather than
being discovered later by a GUI built on the same surface.

`Monitor` is a `DataSink` that draws nothing: it summarises each batch and sends
it down a channel. The display owns all its state on its own thread and is the
only thing that touches the terminal. So the two run at their own rates — a rig
sampling at 1 kHz does not ask for a thousand redraws, and a slow redraw cannot
hold up a write to disk. Nothing is locked anywhere, which is the same
arrangement the device threads already use.

If the display goes away, the sink ignores the failure and the recording carries
on. A monitor failing is no reason to stop a run.

`draw` is kept apart from the loop, so the layout is rendered into a buffer and
checked by tests with no terminal to run in:

```cmd
cargo test -p choptui -- --nocapture
```
