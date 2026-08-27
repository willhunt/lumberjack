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
LUMBERJACK  test_projects/scaled           not recording  record (r)
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

History is kept for every channel, plotted or not. It costs a few kilobytes
each, and it means putting a channel on a plot shows what it has been doing
rather than an empty box that fills up slowly.

Plotting changes nothing about what is recorded. Every channel is written
whether or not it is on a plot: the plots are a window onto data being captured
regardless.

## Recording

`r` starts and stops recording, from any tab. The header counts how long the
current recording has been going:

```
LUMBERJACK  test_projects/scaled              REC 00:01:23  stop (r)
```

Until then nothing is written at all: a project directory that is only being
watched still holds nothing but its `config.json`. The results file is created
when recording starts, not when the display opens.

Stopping and starting again gives a second recording rather than more of the
first. What that means depends on the format the project asks for: a database
keeps both in the one file and its runs table tells them apart, while CSV gets
a file of its own each time, named for when it started.

Devices keep reading whether or not anything is being recorded, so the readings
and the plots carry on regardless.

## Settings

```
┌ Settings ──────────────────────────────────────────────────────────────┐
│                                                                        │
│ >   Plot history      1m                                               │
│     Plot layout       save to plot_config.json                         │
│                                                                        │
└────────────────────────────────────────────────────────────────────────┘
 + and - change a setting.  Enter saves the plot layout.
```

**Plot history** is how far back a plot goes: 10s, 30s, 1m, 2m, 5m, 10m or 30m.
Shortening it takes effect at once rather than once enough new readings have
arrived to push the old ones out.

It is a length of the data, measured from the newest reading rather than from
the clock, so it means the same thing whether the display has been open for ten
seconds or all night. A channel will not hold more than 50,000 readings whatever
the window says — over eight minutes at 100 Hz, under one at 1 kHz — and where
that cap bites, the setting says what is really being held:

```
│ >   Plot history      10m   (holding 47s at this rate)                 │
```

**Plot layout** writes `plot_config.json` beside the project config, and it is
read back the next time the display opens.

## The plot layout file

Kept apart from `config.json`, which describes the rig. What a rig measures and
what somebody happens to be looking at are different things.

```json
{
  "version": 1,
  "history_seconds": 300,
  "plots": [
    { "number": 1, "channels": [ { "device": "Rig", "channel": "Flow" } ] },
    { "number": 3, "channels": [ { "device": "Rig", "channel": "Pressure" } ] }
  ]
}
```

A channel is named the way the library names one, so a plot says which channel
it draws in the same words a calculated channel uses for an input.

The plot number is written out rather than taken from the position in the list.
It is what somebody typed, and renumbering plot 3 to plot 2 to close a gap is
the sort of thing that makes a program feel untrusted.

A layout naming a channel the setup does not have is noted in the log and
skipped, not refused. A layout saved with a device attached should not stop the
display opening when that device is not there.

## Keys

| | |
|---|---|
| `Tab`, `Left`, `Right` | move between tabs |
| `r` | start or stop recording |
| `Up`, `Down` | point at a channel, on the Devices tab |
| `1` to `9` | put the channel on that plot |
| `0` or `-` | take it off |
| `+`, `-` | change a setting, on the Settings tab |
| `Enter` | save the plot layout, on the Settings tab |
| `q`, `Esc`, `Ctrl-C` | stop the run and quit |

## What it does not do yet

Plots have no titles and no fixed axis ranges. Both belong in the layout file
when there is something to set them with; adding a field to it does not need a
version bump, removing or repurposing one does. Note that this is the only thing
writing that file, so if something else starts writing fields of its own they
would be dropped the next time this saves.

If the sink cannot be created when recording starts - a database held open by
something else, a full disk - the error ends the run rather than being reported
and shrugged off. That is what every other sink failure does, but it is a harsh
answer to a key press.

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
