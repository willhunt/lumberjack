# Lumberjack — working agreement

## What this project is

A data acquisition (DAQ) system for both commercial hardware and our own custom DAQ
devices/products.

Repo layout:

- `lumberdaq/` — **the current focus.** Standalone Rust library + binary. Must work
  independently of any GUI.
- `lumbergui/` — the iced interface, and the direction the GUI is taking. There was
  a Tauri + Svelte prototype in `app/`; it was removed once iced settled the question,
  and lives in the history if it is ever wanted.

### Hardware direction

New work goes towards Pico Technology and likely other open-ish DAQ hardware such as
LabJack, not National Instruments. But we still own USB-6001 hardware and still need to
read it, so NI support is wanted — as a legacy option, not a direction.

The old NI backend and the `daqmx-rs` crate were removed in `593796e` because building the
workspace needed NI software installed. `daqmx-rs` ran bindgen from `build.rs` at the time,
putting libclang and the NI SDK on the critical path of every build on every machine. It no
longer does — its bindings are generated once and committed now — but `ni-daqmx-sys` still
emits `cargo:rustc-link-lib=nidaqmx`, so building against it needs `NIDAQmx.lib`, and that
means the NI C development component on every machine that compiles this. Same cost, moved
from bindgen to the linker. Read it for how DAQmx is shaped, by all means; it is MIT and
well organised. Do not depend on it. NI support is
acceptable on the same terms as `picolog` — bindings generated once and committed, the
driver found at runtime with `libloading`, so the workspace builds and runs with no NI
software installed and only someone actually reading a 6001 needs the DAQmx runtime.

Current backends: `MockHardware` (always keep working, it's how we run with no hardware),
`SerialStream`, and `PicoHrdl` via the `picolog` crate. NI is next.

Vendors differ in ways that must stay inside their own crate. A Pico differential input
pairs channel N with N+1, so only odd channels start a pair; NI pairs N with N+4. The
config surface can look the same, the arithmetic cannot be shared.

### Roadmap (rough, not committed)

1. `lumberdaq` as a solid standalone library + CLI. ← we are here
2. A TUI (likely `ratatui`) as a simple first interface that keeps everything in Rust.
3. A GUI. Tauri (reuses the existing Svelte work, web frontend) vs `egui` (pure Rust,
   simpler build, less polished) is **still an open decision** — don't assume either.

Design consequence: keep the library free of UI concerns. No printing to stdout from
library code, no assumptions about a single-threaded event loop, and public types should be
usable from a TUI, a Tauri command, or a headless run alike.

## Primary goal: I am learning Rust

This matters more than shipping fast. I am comfortable with the basics — ownership,
borrowing, `match`, traits, generics at a basic level, `serde` derives. I am *less* solid
on: lifetimes beyond the elided cases, `async`, trait objects vs enums vs generics as a
design choice, error handling beyond `Box<dyn Error>`, interior mutability, and threading.
Assume that level unless I show otherwise, and tell me when something is genuinely
advanced rather than pretending it's routine.

**If you are ever choosing between "write more code" and "explain more", choose explain.**

## How to work with me

### 1. Small changes, one at a time

- Default to **one logical change per turn**, touching **one or two files**. If a task
  needs five files changed, stop and propose the sequence, then do step one.
- Never do a drive-by refactor of code I didn't ask about. If you spot something, mention
  it at the end as a suggestion.
- Prefer showing me a small diff and letting me apply my own judgement over silently
  producing a large working implementation.
- If you find yourself writing more than ~50 lines of new code in a turn, that's a signal
  to stop and check the design with me first.

### 2. Explain what and why, always

After (or ideally before) a change, cover:

- **What** changed, in plain terms.
- **Why** this shape and not another.
- **What Rust concept** is at play, if any — name it so I can go read about it. Link to the
  Book / `std` docs / a specific crate page when it's a concept I said I'm shaky on.
- **What this now makes easy or hard** downstream.

Explanations should be prose, not a wall of bullet points restating the diff. I can read
the diff. Tell me the things the diff doesn't say.

### 3. Discuss options and trade-offs before deciding

For anything with more than one reasonable approach — a data structure, an error strategy,
threading vs async, a crate choice — present the realistic options with their trade-offs,
**give me your recommendation and reasoning**, then let me pick. Don't give me a neutral
survey with no opinion, and don't just pick silently.

Specifically flag when a choice is hard to reverse later (public API shape, the async/sync
boundary, the storage format) versus cheap to change.

### 4. Ask rather than assume

If my request is ambiguous about hardware behaviour, timing requirements, or how a type
should be used from a future UI — ask. Guessing wrong here costs more than a question.

### 5. Teach through review

When I write code and ask for feedback, review it like a Rust-experienced colleague:
point at non-idiomatic patterns, unnecessary clones/allocations, places where the borrow
checker is telling me something about my design rather than just being annoying. Be
direct — I'd rather be told the design is wrong than be handed a polite patch.

## Current architecture (as of this file being written)

Verify against the code before relying on this; it will drift.

```
Daq            — top-level session: info, Vec<Device>, storage paths, sink
 └─ Device     — info, Vec<Channel>, Hardware, ConnectionStatus
     ├─ Channel   — info + accumulated DataPoints
     └─ Hardware  — enum dispatch over concrete backends
```

Decisions already made that you should respect (or explicitly argue against, with
reasoning — they're not sacred):

- **`Hardware` is an `enum`, not `Box<dyn Trait>`.** This was a deliberate refactor
  (commit `0a2e193`). It makes the whole tree `Serialize`/`Deserialize`-able, which is how
  configs round-trip. The cost is that adding a backend means touching the enum and its
  `impl` blocks. The traits `HardwareDataAquisition` and `DeviceInterface` still exist to
  keep the per-backend implementations honest.
- **`serde(tag = "type")`** on `Hardware` so the JSON config self-describes which backend
  a device uses.
- **Errors are `Box<dyn std::error::Error>`** via the `Result`/`Error` aliases in
  `error.rs`. `error.rs` already notes `thiserror` as a candidate. Moving to typed errors
  is a real open question — worth doing as its own focused piece of work, with discussion,
  not as a side effect of another change.
- **Everything is synchronous and single-threaded** right now. Acquisition is a `for` loop
  with `thread::sleep`. This will have to change, and *how* it changes (threads +
  channels vs `async`/tokio) is one of the bigger open decisions. Don't quietly introduce
  `tokio`.

### Known rough edges (don't fix unprompted)

- `ConnectionStatus` variants are `snake_case` and will warn; the reconnect logic in
  `Device::read` calls `connect()` and then returns without reading that cycle.
- `Daq::new` takes a `storage_path` and derives the JSON path from it — storage config is
  fairly tangled with the `Daq` type.
- No tests anywhere in `lumberdaq`. Adding some is a good learning exercise and I'd like to
  do it deliberately.
- `storage_hdf.rs` was deleted; HDF5 output is currently unavailable. Recording
  straight to csv was dropped too, once `lumberdaq export` could write one file
  per run from the database.

## Rust conventions for this repo

- Edition 2021.
- Run `cargo check` / `cargo clippy` and report what they say. Clippy is a good teacher —
  when it fires, explain the lint rather than just silencing it.
- `cargo fmt` is fine to apply to code you touched, but don't reformat whole files you
  otherwise didn't change — it wrecks my ability to read the diff.
- Prefer standard library and well-established crates. Before adding **any** new
  dependency, tell me what it is, why, its rough weight, and whether the std alternative is
  genuinely worse.
- Comments in this codebase sometimes carry reference links (e.g. to serde docs). That's a
  pattern worth continuing for non-obvious decisions.

## Commands

```powershell
cd lumberdaq
cargo run            # runs src/main.rs against examples/simulated_devices/
cargo check
cargo clippy
cargo test
```

Windows is the primary dev platform. The `MockHardware` backend exists so the acquisition
loop can run with no hardware attached at all — keep it working.

## Things not to do

- Don't add async, a new runtime, or a new architectural layer without discussing it first.
- Don't produce a large "here's the whole feature" patch. It defeats the point.
- Don't tell me code works if you haven't run it. Say what you actually verified.
