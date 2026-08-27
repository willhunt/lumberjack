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

## Devices

Every channel in the setup, its latest reading, which plot it is on and how many
readings have arrived. Each device shows whether it is connected, and why not if
it is not.

```
LUMBERJACK  test_projects/scaled                             not recording
Devices  -  Plots  -  Log  -  Settings

┌ Rig ─────────────────────────────── Connected ┐
│                                               │
│ >   Flow         14.500 L/min    [1]  63      │
│     Pressure     7.250 bar       [-]  61      │
│                                               │
└───────────────────────────────────────────────┘

┌ Missing rig ────────────────── port not found ┐
│                                               │
│     Temperature  --- C           [-]  0       │
│                                               │
└───────────────────────────────────────────────┘
```

Devices are listed from the config rather than from arriving data, so a device
that never connects still appears with its channels. That is the one thing worth
looking at when a run will not start.

## Plots

`>` points at a channel. Move it with the up and down arrows, then press `1` to
`9` to put that channel on a plot, or `0` to take it off again. The box beside
the reading is coloured as the line it produces.

```
┌ Plot 1 ──────────────────────────────────────────────────┐
│8.35│                                       ⡠⡀            │ Flow  5.000 L/min
│    │                                      ⡔⠁⠈⠑⢄⡀         │
│    │                                    ⢀⠎     ⠈⠢⢄       │
│    │                                   ⢠⠊         ⠑⠤⡀    │
│    │                                  ⡰⠁            ⠈⠒⢄  │
│    │           ⢀⡠⠒⠤⣀              ⡰⠁                     │
│    │         ⡠⠔⠁    ⠉⠒⠤⣀         ⡔⠁                      │
│    │      ⢀⠔⠊           ⠉⠒⠤⣀   ⢀⠎                        │
│    │    ⡠⠒⠁                 ⠉⠒⠤⠃                         │
│0.65│⠊                                                    │
│    └─────────────────────────────────────────────────────│
│ 0.0s                                                 4.0s│
└──────────────────────────────────────────────────────────┘
```

Several channels can share a plot, and the legend beside it is also the reading,
so watching a plot does not mean switching back to Devices for the numbers.

The time axis counts from the first reading of the run rather than from when the
display started, so it says when a reading was taken and not when it was drawn.

A channel keeps its last 600 readings, whether or not it is plotted — it costs a
few kilobytes, and it means putting a channel on a plot shows what it has been
doing rather than an empty box that fills up slowly. That also means the window a
plot covers is set by the sample rate: 600 readings is twelve seconds at 50 Hz
and half a second at 1 kHz. The axis says which.

Plot assignments last as long as the session. Saving them is what the Settings
tab is for, and it is not built yet.

## Keys

| | |
|---|---|
| `Tab`, `Left`, `Right` | move between tabs |
| `Up`, `Down` | point at a channel, on the Devices tab |
| `1` to `9` | put the channel on that plot |
| `0` or `-` | take it off |
| `q`, `Esc`, `Ctrl-C` | stop the run and quit |

## What it does not do yet

**Nothing is recorded.** It connects, reads and displays. Deliberately not
`lumberdaq::open`, which attaches the project's sink and so creates the results
file before anything has been recorded — watching a rig should leave nothing
behind. The record button in the header is the next piece.

Settings is empty.

## How it is put together

A separate crate rather than part of lumberdaq, so it can only reach the public
API. Anything it needs that is not exported fails to compile here rather than
being discovered later by a GUI built on the same surface.

`Monitor` is a `DataSink` that draws nothing: it hands each batch to a channel
and the display owns all its state on its own thread, where it is the only thing
that touches the terminal. So the two run at their own rates — a rig sampling at
1 kHz does not ask for a thousand redraws, and a slow redraw cannot hold up a
write to disk. Nothing is locked anywhere, which is the same arrangement the
device threads already use.

Every reading is sent, not just the newest. A number on screen only needs the
last one, but a plot drawn from one point per batch is a picture of the drain
rate rather than of the signal.

If the display goes away, the sink ignores the failure and the recording carries
on. A monitor failing is no reason to stop a run.

`draw` is kept apart from the loop, so the layout is rendered into a buffer and
checked by tests with no terminal to run in:

```cmd
cargo test -p choptui -- --nocapture
```
