use chrono::{DateTime, Utc};
use iced::widget::pane_grid::{self, Axis, Configuration, PaneGrid};
use iced::widget::{
    button, checkbox, column, container, opaque, pick_list, row, scrollable, space, stack,
    slider, text, text_input, tooltip, MouseArea, Row,
};
use iced::window;
mod settings;

use crate::settings::UserSettings;
use iced::event::{self, Event, Status};
use iced::mouse;
use iced::{padding, Border, Bottom, Center, Color, Element, Fill, Point, Subscription, Theme, Top};
// use iced::border;
use iced_fonts::lucide::*;
use iced_plot::{
    LineStyle, PlotStyle, PlotUiMessage, PlotWidget, PlotWidgetBuilder, Series, ShapeId,
};
use lumberdaq::calculated::ChannelRef;
use lumberdaq::channel::{Channel, Scale};
use lumberdaq::config::DaqConfig;
use lumberdaq::daq::DaqInfo;
use lumberdaq::datapoint::DataPoint;
// Qualified rather than imported: `Acquisition` here is this file's own struct
// for a run in progress, and a mock device's way of sampling is a different
// thing that happens to share the word.
use lumberdaq::hardware::mock_hardware::{self, MockHardwareInput};
use lumberdaq::hardware::{pico_hrdl, serial_stream};
use lumberdaq::hardware::HardwareConfig;
use lumberdaq::plot_config::{self, PlotLayout, SplitAxis};
use lumberdaq::project::Project;
use lumberdaq::session::DeviceEvent;
use lumberdaq::storage::{Batch, DataSink, Fanout, Recorder};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// How much of the recent past a plot shows comes from the saved layout's
/// `history_seconds`.
///
/// However long it is, the viewport is slid by wall-clock time on every frame
/// while points are only ever added when real samples arrive. Keeping those
/// two apart is what makes the scroll continuous without drawing any value
/// that wasn't measured: between batches the line simply stops short of the
/// right edge.
///
/// Enough colours to tell a handful of traces apart. Cycled if there are more channels
/// than colours, which is a legend problem to solve when it happens.
const PALETTE: [Color; 6] = [
    Color::from_rgb(0.30, 0.70, 1.00),
    Color::from_rgb(1.00, 0.50, 0.20),
    Color::from_rgb(0.45, 0.85, 0.45),
    Color::from_rgb(0.95, 0.45, 0.75),
    Color::from_rgb(0.85, 0.80, 0.35),
    Color::from_rgb(0.65, 0.55, 1.00),
];

/// How many batches may be waiting for the interface before they start being
/// dropped. See `DisplaySink::write_batch` for why dropping is the answer.
const CHANNEL_DEPTH: usize = 256;

/// How far back a plot goes when its project has no saved layout to say.
const DEFAULT_HISTORY_SECONDS: u64 = 5;

/// Corner rounding shared by every field, so they look like one family.
const FIELD_RADIUS: f32 = 6.0;

/// How much of the window the sidebar takes, and how it divides between the
/// device list and the settings below it.
const SIDEBAR_WIDTH: f32 = 0.3;
const SIDEBAR_SPLIT: f32 = 0.5;

/// Where the log's divider sits when it is open, and when it is shut.
///
/// Shut leaves just enough for its own title row, which is what makes it
/// openable again.
const LOG_OPEN: f32 = 0.78;
const LOG_SHUT: f32 = 0.95;

/// How long a plot keeps readings for, to choose between.
///
/// Steps rather than a free number: the point of the setting is how far back a
/// plot goes, and a handful of round answers covers that better than typing
/// seconds. The same list choptui offers, so the two interfaces do not
/// disagree about what the sensible spans are.
const HISTORY_CHOICES: [u64; 7] = [10, 30, 60, 120, 300, 600, 1800];

/// How often to collect from a device, to choose between.
///
/// Stepped for the same reason the history is, and because this is a rate a
/// rig is run at rather than a number to tune: a device read every 100 ms or
/// every second is a decision, 137 ms is a typo.
const READ_INTERVALS: [u64; 8] = [10, 20, 50, 100, 200, 500, 1000, 5000];

/// How long to wait after the last change before writing a project out.
///
/// Saving on every change would mean a file write per keystroke while
/// something is being renamed. Waiting for the typing to stop turns that into
/// one write, without asking anybody to remember to press save.
const SAVE_DELAY: Duration = Duration::from_secs(1);

/// A rig with nothing in it, for when no project is open.
///
/// Not a failure state: a setup with no devices is a setup, and treating it as
/// one is what keeps every panel written as though a project were open.
fn empty_config() -> DaqConfig {
    DaqConfig {
        info: lumberdaq::daq::DaqInfo { name: String::new(), author: String::new() },
        storage: lumberdaq::config::StorageFormat::default(),
        devices: Vec::new(),
        calculated: None,
    }
}

/// How the window is divided, before anybody has dragged anything.
///
/// The log runs the full width along the bottom, under both columns, rather
/// than sitting inside either of them: it reports on the run as a whole, not
/// on the devices or the plots.
fn default_panes() -> pane_grid::State<PaneKind> {
    pane_grid::State::with_configuration(Configuration::Split {
        axis: Axis::Horizontal,
        // Shut to begin with: the log is worth having when something has gone
        // wrong, and worth nothing the rest of the time.
        ratio: LOG_SHUT,
        a: Box::new(Configuration::Split {
            axis: Axis::Vertical,
            ratio: SIDEBAR_WIDTH,
            a: Box::new(Configuration::Split {
                axis: Axis::Horizontal,
                ratio: SIDEBAR_SPLIT,
                a: Box::new(Configuration::Pane(PaneKind::Devices)),
                b: Box::new(Configuration::Pane(PaneKind::Config)),
            }),
            b: Box::new(Configuration::Pane(PaneKind::Data)),
        }),
        b: Box::new(Configuration::Pane(PaneKind::Log)),
    })
}

fn devices_from(config: &DaqConfig) -> Vec<AppDevice> {
    config
        .devices
        .iter()
        .map(|device| AppDevice {
            name: device.info.name.clone(),
            channels: device
                .hardware
                .channel_infos()
                .into_iter()
                .map(|channel| AppChannel {
                    name: channel.name,
                    unit: channel.unit,
                    latest: None,
                    samples: 0,
                })
                .collect(),
            expanded: false,
        })
        .collect()
}

/// What the acquisition thread has to say. Data and status share one channel
/// so they arrive in the order they happened, the same reason `DeviceMessage`
/// does it that way inside lumberdaq.
enum FromAcquisition {
    Data { device: String, channel: String, datapoints: Vec<DataPoint> },
    Status(String),
}

/// A `DataSink` feeding the interface instead of a file.
///
/// A sink is how lumberdaq already offers data to something other than
/// storage — `Fanout`'s documentation calls this case out as "a display fed
/// the same data as the file it is being written to" — so watching a run
/// needs no change to the library.
struct DisplaySink {
    to_interface: SyncSender<FromAcquisition>,
}

impl DataSink for DisplaySink {
    fn init(&mut self, _config: &DaqConfig) -> lumberdaq::Result<()> {
        Ok(())
    }

    /// Never fails, on purpose.
    ///
    /// `Fanout` reports a failing sink and that ends the run, which is the
    /// right answer for a disk that has filled up and the wrong one for a
    /// window somebody closed. So a full queue drops the batch and a vanished
    /// receiver is ignored: the display losing points costs nothing, while
    /// blocking here would stall collection, and the writing to disk behind it
    /// in a run that was recording.
    fn write_batch(&mut self, batch: &Batch) -> lumberdaq::Result<()> {
        let _ = self.to_interface.try_send(FromAcquisition::Data {
            device: batch.device.clone(),
            channel: batch.channel.clone(),
            datapoints: batch.datapoints.clone(),
        });
        Ok(())
    }

    fn flush(&mut self) -> lumberdaq::Result<()> {
        Ok(())
    }
}

/// A run in progress: how to hear from it, how to stop it, and how to tell
/// when it has finished stopping.
struct Acquisition {
    from_acquisition: Receiver<FromAcquisition>,
    /// Set to end the run. Read only by the acquisition thread.
    stop: Arc<AtomicBool>,
    /// Set to record what is being read. The `Recorder` on the other side
    /// follows it: raising it builds a sink, lowering it flushes and drops
    /// one, so watching a run leaves no results file behind at all.
    recording: Arc<AtomicBool>,
    thread: thread::JoinHandle<()>,
}

/// Whether a run is going, and whether one is on its way out.
///
/// Stopping is not instant: the thread notices the flag when it next comes
/// round its read loop, so there is a moment where the run is neither going
/// nor finished. Starting another one before the last has let go of its
/// devices would mean two threads holding the same serial port.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RunState {
    Running,
    Stopping,
    Stopped,
}

/// Run the acquisition on a thread of its own, reporting back over a channel.
///
/// The `Daq` stays on that thread: `run` takes `&mut self` and blocks until
/// stopped, so while a run is in progress the devices are unreachable from
/// here. Everything the interface knows arrives through the channel, which is
/// why the device list is built from the config rather than from live devices.
fn start_acquisition(config: DaqConfig, directory: PathBuf) -> Acquisition {
    let (sender, receiver) = mpsc::sync_channel(CHANNEL_DEPTH);
    let stop = Arc::new(AtomicBool::new(false));
    let stop_in_thread = Arc::clone(&stop);
    let recording = Arc::new(AtomicBool::new(false));
    let recording_in_thread = Arc::clone(&recording);
    // Read before the config is handed over, so the recorder knows what format
    // to write when somebody eventually asks it to.
    let storage = config.storage;

    let thread = thread::spawn(move || {
        let report = |text: String| {
            let _ = sender.try_send(FromAcquisition::Status(text));
        };

        // from_config rather than lumberdaq::open: open attaches the project's
        // storage sink up front, which creates the results file whether or not
        // anybody records. The Recorder below builds one only when armed.
        let mut daq = match lumberdaq::daq::Daq::from_config(config) {
            Ok(daq) => daq,
            Err(error) => return report(format!("could not build the setup: {}", error)),
        };

        let recorder = Recorder::new(
            recording_in_thread,
            Box::new(move || {
                // Labelled by when it started, which a CSV needs to get a file
                // of its own. A database keeps every run in the one file and
                // tells them apart by its own runs table, so it ignores this.
                let label = Utc::now().format("%Y%m%d-%H%M%S").to_string();
                Project::new(&directory).sink_for(storage, &label)
            }),
        );

        // A run has one sink, so watching *and* recording means a sink that is
        // both. The display is first: a storage failure ends the run, and the
        // interface should have been given the batch before that happens.
        let sink = Fanout::new()
            .and("display", Box::new(DisplaySink { to_interface: sender.clone() }))
            .and("recording", Box::new(recorder));

        if let Err(error) = daq.set_sink(Box::new(sink)) {
            return report(format!("could not attach the sinks: {}", error));
        }

        let connected = daq.connect();
        report(format!(
            "{} of {} devices connected",
            connected.connected.len(),
            daq.devices.len()
        ));

        let outcome = daq.run(&stop_in_thread, &mut |event| {
            let _ = sender.try_send(FromAcquisition::Status(match event {
                DeviceEvent::Problem { device, error } => format!("{}: {}", device, error),
                DeviceEvent::Connected { device } => format!("{} came back", device),
                DeviceEvent::Disconnected { device, cause } => {
                    format!("lost {}: {}", device, cause.unwrap_or_default())
                }
            }));
        });

        match outcome {
            Ok(()) => report("run finished".to_string()),
            Err(error) => report(format!("run ended: {}", error)),
        }
    });

    Acquisition { from_acquisition: receiver, stop, recording, thread }
}

/// The trace one channel is drawn as.
///
/// A series cannot be built empty and no data has arrived for a channel just
/// put on a plot, so each starts with a single point far enough in the past to
/// be outside any viewport. The first real batch trims it away.
fn new_series(reference: &ChannelRef, colour: Color) -> Series {
    Series::line_only(vec![[f64::MIN / 2.0, 0.0]], LineStyle::solid())
        .with_label(reference.channel.clone())
        .with_color(colour)
}

/// Turn a saved arrangement into one iced can lay out, dropping any plot that
/// is no longer there.
///
/// A split that loses one half becomes whatever is left of it rather than a
/// gap, so deleting a plot elsewhere closes up the arrangement instead of
/// leaving a hole in it.
fn configuration_from(layout: &PlotLayout, known: &[usize]) -> Option<Configuration<usize>> {
    match layout {
        PlotLayout::Plot { number } => {
            known.contains(number).then_some(Configuration::Pane(*number))
        }
        PlotLayout::Split { axis, ratio, first, second } => {
            match (configuration_from(first, known), configuration_from(second, known)) {
                (Some(a), Some(b)) => Some(Configuration::Split {
                    axis: match axis {
                        SplitAxis::Horizontal => Axis::Horizontal,
                        SplitAxis::Vertical => Axis::Vertical,
                    },
                    ratio: *ratio,
                    a: Box::new(a),
                    b: Box::new(b),
                }),
                (Some(only), None) | (None, Some(only)) => Some(only),
                (None, None) => None,
            }
        }
    }
}

/// Plots stacked one above another, for a project nobody has arranged.
///
/// Equal shares: the first of three gets a third, and what is left is divided
/// again below it.
fn stacked(numbers: &[usize]) -> Option<Configuration<usize>> {
    let (first, rest) = numbers.split_first()?;
    match stacked(rest) {
        None => Some(Configuration::Pane(*first)),
        Some(rest) => Some(Configuration::Split {
            axis: Axis::Horizontal,
            ratio: 1.0 / numbers.len() as f32,
            a: Box::new(Configuration::Pane(*first)),
            b: Box::new(rest),
        }),
    }
}

/// Read an arrangement back out of the grid, to be saved.
fn layout_from(node: &pane_grid::Node, panes: &pane_grid::State<usize>) -> Option<PlotLayout> {
    match node {
        pane_grid::Node::Pane(pane) => {
            panes.get(*pane).map(|number| PlotLayout::Plot { number: *number })
        }
        pane_grid::Node::Split { axis, ratio, a, b, .. } => {
            match (layout_from(a, panes), layout_from(b, panes)) {
                (Some(first), Some(second)) => Some(PlotLayout::Split {
                    axis: match axis {
                        Axis::Horizontal => SplitAxis::Horizontal,
                        Axis::Vertical => SplitAxis::Vertical,
                    },
                    ratio: *ratio,
                    first: Box::new(first),
                    second: Box::new(second),
                }),
                (Some(only), None) | (None, Some(only)) => Some(only),
                (None, None) => None,
            }
        }
    }
}

/// Build one plot: a trace per channel, and the widget to draw them on.
///
/// The channels are expected to have been checked against the setup already;
/// anything left here gets a trace.
fn build_plot(
    number: usize,
    name: String,
    channels: Vec<ChannelRef>,
    window_seconds: f64,
) -> AppPlot {
    let mut builder = PlotWidgetBuilder::new()
        .with_x_label("Time (s)")
        // Left to autoscale: these are real channels in real units, so there
        // is no sensible fixed range to impose. The x axis is set below
        // instead, which overrides autoscaling for that axis alone.
        .with_autoscale_on_updates(true)
        // Picking a point out of a trace that is sliding leftwards is chasing
        // a moving target, and the highlight covers the data while you do it.
        .with_highlight_on_hover(false)
        // Drawn as ordinary widgets above the plot instead, since the built-in
        // one sits over the y axis and its position is not something
        // iced_plot exposes.
        .disable_legend()
        // Both the frame and the canvas behind it take the card's colour, so a
        // plot reads as part of its card rather than as a panel sitting on
        // one. Everything else is left to iced_plot's theme-derived defaults.
        .with_style(|theme: &Theme| PlotStyle {
            frame: container::background(card_colour(theme)),
            plot_area: container::background(card_colour(theme)),
            ..iced_plot::default_style(theme)
        });

    let mut plotted = Vec::new();
    for (index, reference) in channels.into_iter().enumerate() {
        let colour = PALETTE[index % PALETTE.len()];
        let series = new_series(&reference, colour);

        plotted.push(PlottedChannel { reference, colour, series: series.id });
        builder = builder.add_series(series);
    }

    let widget = match plotted.is_empty() {
        true => None,
        false => {
            let mut widget = builder.build().expect("a plot of the channels it was given");
            // The in-canvas `?` and its panel: help for a widget's own mouse
            // controls, sitting on top of the data in every plot on screen.
            widget.set_controls_help(false);
            widget.set_x_lim(-window_seconds, 0.0);
            Some(widget)
        }
    };

    AppPlot { number, name, channels: plotted, widget }
}

/// The fill behind anything that can be typed into or chosen from.
///
/// One colour for text fields, dropdowns and the lists that hold them, so a
/// panel reads as a set of inputs rather than as several unrelated widgets.
fn field_colour(theme: &Theme) -> Color {
    theme.extended_palette().background.weakest.color
}

/// Text fields: the shared fill, and corners rounded to match everything else.
fn field_style(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let palette = theme.extended_palette();
    let mut style = text_input::default(theme, status);

    style.background = iced::Background::Color(field_colour(theme));
    style.border = Border {
        radius: FIELD_RADIUS.into(),
        width: 1.0,
        color: match status {
            text_input::Status::Focused { .. } => palette.primary.base.color,
            _ => palette.background.weak.color,
        },
    };
    style
}

/// Dropdowns, styled to match the text fields beside them.
fn field_pick_style(theme: &Theme, status: pick_list::Status) -> pick_list::Style {
    let palette = theme.extended_palette();
    let mut style = pick_list::default(theme, status);

    style.background = iced::Background::Color(field_colour(theme));
    style.border = Border {
        radius: FIELD_RADIUS.into(),
        width: 1.0,
        color: match status {
            pick_list::Status::Hovered | pick_list::Status::Opened { .. } => {
                palette.primary.base.color
            }
            _ => palette.background.weak.color,
        },
    };
    style
}

/// The label above a field: smaller than what it labels, and quieter, so the
/// value is what the eye lands on.
fn field_label(label: &str) -> Element<'_, Message> {
    text(label)
        .size(12)
        // Softened from the theme's own text colour rather than a fixed grey,
        // which would be invisible on one theme and harsh on another.
        .style(|theme: &Theme| text::Style {
            color: Some(Color { a: 0.7, ..theme.extended_palette().background.base.text }),
        })
        .into()
}

/// One line of a menu: something to do, or a rule between groups of them.
enum MenuItem<'a> {
    /// A label and what it does. `None` is shown greyed rather than hidden, so
    /// the menu keeps its shape and its items do not move about — an entry
    /// being unavailable is worth saying, where a menu that silently loses one
    /// just looks different.
    Entry(&'a str, Option<Message>),
    Divider,
}

/// The menu a right click opens: a short list of things to do to one item.
fn context_menu<'a>(entries: Vec<(&'a str, Option<Message>)>) -> Element<'a, Message> {
    menu(
        entries.into_iter().map(|(label, message)| MenuItem::Entry(label, message)).collect(),
        150.0,
    )
}

/// How a dialog is drawn: a panel lifted clear of the interface behind it.
///
/// Shared so the three of them cannot drift apart — they are the same kind of
/// thing and should not be three slightly different panels.
fn dialog_style(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();

    container::Style {
        background: Some(palette.background.base.color.into()),
        border: Border {
            radius: 8.0.into(),
            width: 1.0,
            color: palette.background.strong.color,
        },
        ..container::Style::default()
    }
}

/// A floating list of things to do.
fn menu<'a>(items: Vec<MenuItem<'a>>, width: f32) -> Element<'a, Message> {
    container(
        column(items.into_iter().map(|item| match item {
            MenuItem::Entry(label, message) => button(text(label).size(13))
                .style(button::text)
                .padding([2, 6])
                .width(Fill)
                .on_press_maybe(message)
                .into(),
            // Groups what is above it apart from what is below, which is the
            // whole of its job.
            MenuItem::Divider => container(space::horizontal().height(1))
                .width(Fill)
                .padding(padding::top(2).bottom(2))
                .style(|theme: &Theme| container::Style {
                    background: Some(
                        theme.extended_palette().background.strong.color.into(),
                    ),
                    ..container::Style::default()
                })
                .into(),
        }))
        .spacing(2),
    )
    // Wide enough for its longest entry and no wider. Without this the menu
    // takes whatever the layer it floats on offers, which is the window.
    .width(width)
    .padding(4)
    .style(|theme: &Theme| {
        let palette = theme.extended_palette();

        container::Style {
            background: Some(palette.background.base.color.into()),
            border: Border {
                radius: FIELD_RADIUS.into(),
                width: 1.0,
                color: palette.background.strong.color,
            },
            ..container::Style::default()
        }
    })
    .into()
}

/// The soft line under a pane's heading.
///
/// A free function rather than a method: it borrows nothing, and as a method
/// its result would be tied to the borrow of `self`, which is shorter than the
/// widgets it has to sit alongside.
fn pane_rule<'a>() -> Element<'a, Message> {
    container(space::horizontal().height(1))
        .width(Fill)
        .style(|theme: &Theme| container::Style {
            background: Some(
                Color { a: 0.4, ..theme.extended_palette().background.strong.color }.into(),
            ),
            ..container::Style::default()
        })
        .into()
}

/// The bubble a tooltip is drawn in. Solid, so what is underneath does not
/// show through the explanation.
fn tip_style(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();

    container::Style {
        background: Some(palette.background.base.color.into()),
        text_color: Some(palette.background.base.text),
        border: Border {
            radius: FIELD_RADIUS.into(),
            width: 1.0,
            color: palette.background.strong.color,
        },
        ..container::Style::default()
    }
}

/// One of the three transport buttons.
///
/// Always the same three, always in the same place. What changes is whether
/// this one is lit: the colour says what the rig is doing, where the icon says
/// what the button is for.
///
/// A function with a named lifetime rather than a closure, because `Tooltip`
/// is invariant over its lifetime — a closure taking `Element<'_, _>` gets a
/// fresh lifetime that cannot then be shortened to match what it returns.
fn transport<'a>(
    icon: Element<'a, Message>,
    lit: bool,
    // Spelled as a function pointer because each `button::*` style is its own
    // distinct type, so passing one would otherwise fix the type for all three.
    colour: fn(&Theme, button::Status) -> button::Style,
    tip: &'a str,
    message: Message,
) -> Element<'a, Message> {
    tooltip(
        button(icon)
            .style(match lit {
                true => colour,
                false => button::secondary,
            })
            .padding(6)
            .on_press(message),
        container(text(tip).size(13)).padding(4).style(tip_style),
        tooltip::Position::Bottom,
    )
    .into()
}

/// An icon-only control, with the words it hasn't got.
///
/// A button showing only a symbol is quick to use once you know it and opaque
/// until then, and a tooltip is how it stops being opaque without taking the
/// room a label would.
///
/// A free function rather than a method for the same reason `transport` is:
/// `Tooltip` is invariant in its lifetime, so building one inside a method
/// would tie the result to the borrow of `self` rather than to the interface
/// it belongs in.
fn hint<'a>(control: impl Into<Element<'a, Message>>, tip: &'a str) -> Element<'a, Message> {
    tooltip(
        control,
        container(text(tip).size(13)).padding(4).style(tip_style),
        tooltip::Position::Bottom,
    )
    .into()
}

/// A label with its explanation behind it, rather than beneath it.
///
/// The paragraph a setting needs to be understood is worth having and not
/// worth the room it takes once it has been read, which is what a tooltip is
/// for.
fn labelled<'a>(label: &'a str, explanation: &'a str) -> Element<'a, Message> {
    tooltip(
        field_label(label),
        container(text(explanation).size(13)).padding(6).max_width(260).style(tip_style),
        tooltip::Position::Right,
    )
    .into()
}

/// The background a plot's card is drawn on.
///
/// One definition because the card and the plot inside it have to agree: the
/// point of the card is that a plot and its legend look like one thing.
fn card_colour(theme: &Theme) -> Color {
    theme.extended_palette().background.weaker.color
}

/// A device name nothing in the setup is using yet.
///
/// Names are how a batch finds its way back to the channel that produced it,
/// so two devices sharing one would send their data to the same place.
fn unused_name(wanted: &str, config: &DaqConfig) -> String {
    let taken = |name: &str| config.devices.iter().any(|device| device.info.name == name);

    if !taken(wanted) {
        return wanted.to_string();
    }
    (2..)
        .map(|suffix| format!("{} {}", wanted, suffix))
        .find(|name| !taken(name))
        .unwrap_or_else(|| wanted.to_string())
}

/// A channel name this device is not already using.
fn unused_channel_name(wanted: &str, existing: &[String]) -> String {
    if !existing.iter().any(|name| name == wanted) {
        return wanted.to_string();
    }
    (2..)
        .map(|suffix| format!("{} {}", wanted, suffix))
        .find(|name| !existing.contains(name))
        .unwrap_or_else(|| wanted.to_string())
}

/// Find one channel by the pair of names that identifies it.
///
/// By name because that is all a `Batch` carries, and all a `ChannelRef` is.
fn find_channel<'a>(
    devices: &'a mut [AppDevice],
    device_name: &str,
    channel_name: &str,
) -> Option<&'a mut AppChannel> {
    devices
        .iter_mut()
        .find(|device| device.name == device_name)?
        .channels
        .iter_mut()
        .find(|channel| channel.name == channel_name)
}

/// Convert an acquired timestamp to the plot's x axis: seconds from a
/// reference taken when the interface started.
///
/// The axis is driven by one clock throughout — `Utc`, the same one the
/// datapoints carry — so the sliding viewport and the points inside it cannot
/// drift apart. It does mean the axis inherits the wall clock's habits; a
/// machine that steps its clock mid-run would step the plot with it.
fn seconds_since(reference: DateTime<Utc>, moment: DateTime<Utc>) -> f64 {
    (moment - reference).num_microseconds().unwrap_or(0) as f64 / 1_000_000.0
}

pub fn main() -> iced::Result {
    iced::application(AppDaq::new, AppDaq::update, AppDaq::view)
        .subscription(AppDaq::subscription)
        .theme(AppDaq::theme)
        .scale_factor(AppDaq::scale_factor)
        .font(iced_fonts::LUCIDE_FONT_BYTES)
        .run()
}

struct AppChannel {
    name: String,
    unit: String,
    /// The most recent value acquired, once any has been.
    latest: Option<f64>,
    samples: usize,
}
struct AppDevice {
    name: String,
    channels: Vec<AppChannel>,
    expanded: bool,
}

/// One channel drawn on a plot, and the colour it is drawn in.
///
/// A reference rather than a copy: the readings belong to the channel on its
/// device, and `ChannelRef` is how lumberdaq itself names one channel of one
/// device — the same pair of names a `Batch` arrives carrying. The colour
/// lives here so the legend and the trace cannot disagree about it.
struct PlottedChannel {
    reference: ChannelRef,
    colour: Color,
    /// The trace this channel is drawn as on this plot. A channel on two plots
    /// has a series on each, which is why this lives here and not on the
    /// channel itself.
    series: ShapeId,
}

/// A Pico input range, named as somebody reading a datasheet would name it.
///
/// A wrapper because the range itself belongs to picolog and cannot be given a
/// `Display` here, and because "±2500 mV" is what the setting means where
/// `MilliVolts2500` is only what it is called.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PicoRange(pico_hrdl::VoltageRange);

impl std::fmt::Display for PicoRange {
    fn fmt(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str(match self.0 {
            pico_hrdl::VoltageRange::MilliVolts2500 => "±2500 mV",
            pico_hrdl::VoltageRange::MilliVolts1250 => "±1250 mV",
            pico_hrdl::VoltageRange::MilliVolts625 => "±625 mV",
            pico_hrdl::VoltageRange::MilliVolts313 => "±313 mV",
            pico_hrdl::VoltageRange::MilliVolts156 => "±156 mV",
            pico_hrdl::VoltageRange::MilliVolts78 => "±78 mV",
            pico_hrdl::VoltageRange::MilliVolts39 => "±39 mV",
        })
    }
}

/// Every Pico range, widest first, as they are offered.
const PICO_RANGES: [PicoRange; 7] = [
    PicoRange(pico_hrdl::VoltageRange::MilliVolts2500),
    PicoRange(pico_hrdl::VoltageRange::MilliVolts1250),
    PicoRange(pico_hrdl::VoltageRange::MilliVolts625),
    PicoRange(pico_hrdl::VoltageRange::MilliVolts313),
    PicoRange(pico_hrdl::VoltageRange::MilliVolts156),
    PicoRange(pico_hrdl::VoltageRange::MilliVolts78),
    PicoRange(pico_hrdl::VoltageRange::MilliVolts39),
];

/// The spans an NI analog input is usually asked for, smallest last.
///
/// A pair rather than a single number because the config takes a span, and one
/// that is not symmetric is perfectly legal.
const NI_RANGES: [(f64, f64); 5] =
    [(-10.0, 10.0), (-5.0, 5.0), (-2.0, 2.0), (-1.0, 1.0), (-0.2, 0.2)];

/// An NI span, written the way a range is written.
#[derive(Debug, Clone, Copy, PartialEq)]
struct NiRange((f64, f64));

impl std::fmt::Display for NiRange {
    fn fmt(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(formatter, "{} to {} V", self.0 .0, self.0 .1)
    }
}

/// The speeds a serial device is likely to be running at.
const BAUD_RATES: [u32; 8] = [9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600];

/// How often a streaming mock device produces a sample, to choose between.
const MOCK_INTERVALS: [u64; 9] = [1, 2, 5, 10, 20, 50, 100, 500, 1000];

/// Which of a mock device's ways of sampling is in use.
///
/// The variant without its settings, so it can be offered as a choice: an
/// `Acquisition` carries the interval, and picking a mode should not mean
/// picking an interval at the same time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MockMode {
    Polled,
    Streaming,
}

impl std::fmt::Display for MockMode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str(match self {
            MockMode::Polled => "Polled",
            MockMode::Streaming => "Streaming",
        })
    }
}

/// Which kind of value a mock channel generates, without the value itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MockInputKind {
    Random,
    Constant,
    Sine,
}

impl std::fmt::Display for MockInputKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str(match self {
            MockInputKind::Random => "Random",
            MockInputKind::Constant => "Constant",
            MockInputKind::Sine => "Sine",
        })
    }
}

impl MockInputKind {
    fn of(input: &MockHardwareInput) -> MockInputKind {
        match input {
            MockHardwareInput::Random => MockInputKind::Random,
            MockHardwareInput::Constant(_) => MockInputKind::Constant,
            MockHardwareInput::Sine { .. } => MockInputKind::Sine,
        }
    }

    /// The number this kind carries, where it carries one.
    fn number(input: &MockHardwareInput) -> Option<f64> {
        match input {
            MockHardwareInput::Random => None,
            MockHardwareInput::Constant(value) => Some(*value),
            MockHardwareInput::Sine { frequency_hz } => Some(*frequency_hz),
        }
    }

    /// What that number is called, for the field's label.
    fn number_label(&self) -> Option<&'static str> {
        match self {
            MockInputKind::Random => None,
            MockInputKind::Constant => Some("Value"),
            MockInputKind::Sine => Some("Frequency (Hz)"),
        }
    }
}

/// Where the pointer was last seen, in window coordinates.
///
/// Outside the state, and deliberately: `on_right_press` says only that a
/// right click happened, never where, and a menu that floats over the
/// interface has to be put somewhere. Held in the interface's own state it
/// would need a message per mouse movement, and iced rebuilds the whole widget
/// tree for every message — so moving the mouse would have cost more than
/// running the acquisition does.
///
/// A `Mutex` rather than a `thread_local!` because subscriptions run on the
/// executor, which by default is a thread pool: the writer and the reader are
/// not promised to be the same thread. One uncontended lock per mouse movement
/// is nothing next to a relayout.
static POINTER: Mutex<Point> = Mutex::new(Point::ORIGIN);

/// Where the pointer is, for whoever needs to put something there.
///
/// A poisoned lock would mean a panic while holding it, which cannot happen
/// here: nothing between the lock and the unlock can fail. Recovering the
/// value is still better than joining the panic.
fn pointer() -> Point {
    match POINTER.lock() {
        Ok(seen) => *seen,
        Err(poisoned) => *poisoned.into_inner(),
    }
}

/// Follow the pointer without telling the interface about it.
///
/// Always `None`: returning a message here is exactly what this exists to
/// avoid. The right click itself still arrives the ordinary way, through the
/// `on_right_press` on the row that was clicked, and reads the position back
/// out of `POINTER` once.
fn watch_pointer(event: Event, _status: Status, _window: window::Id) -> Option<Message> {
    if let Event::Mouse(mouse::Event::CursorMoved { position }) = event {
        if let Ok(mut seen) = POINTER.lock() {
            *seen = position;
        }
    }
    None
}

/// What a right click was on, while its menu is open.
///
/// Says what the menu acts on. Where it is drawn is `context_at`, taken from
/// `POINTER` at the moment of the click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContextMenu {
    Device(usize),
    Channel(usize, usize),
    Plot(usize),
}

/// What is being configured in the settings panel.
///
/// Only plots so far; devices and channels are the same idea and will join it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Selection {
    Plot(usize),
    /// The settings that apply to every plot rather than to one of them.
    AllPlots,
    Device(usize),
    /// A channel, by the device it is on and its position in that device.
    Channel(usize, usize),
}

/// Which channels a plot draws, and what it is called.
struct AppPlot {
    /// As written in the layout file. Kept rather than taken from the position
    /// in the list, so saving does not renumber somebody's plots behind their
    /// back when one in the middle is deleted.
    number: usize,
    name: String,
    channels: Vec<PlottedChannel>,
    /// None until the plot has a channel on it: there is no such thing as a
    /// plot of nothing, and `PlotWidgetBuilder` will not build one.
    widget: Option<PlotWidget>,
}

impl AppPlot {
    /// Put a channel on this plot.
    ///
    /// The trace is added to the widget rather than the whole plot being
    /// rebuilt, so the channels already on it keep the data they have
    /// collected. A plot that had nothing on it has no widget yet, so this is
    /// where it gets one.
    fn add_channel(&mut self, reference: ChannelRef, window_seconds: f64) {
        if self.channels.iter().any(|plotted| plotted.reference == reference) {
            return;
        }

        let colour = self.next_colour();
        let series = new_series(&reference, colour);
        let id = series.id;

        match self.widget.as_mut() {
            Some(widget) => {
                if widget.add_series(series).is_err() {
                    return;
                }
            }
            None => {
                let plot = build_plot(
                    self.number,
                    self.name.clone(),
                    vec![reference.clone()],
                    window_seconds,
                );
                self.widget = plot.widget;
                self.channels = plot.channels;
                return;
            }
        }

        self.channels.push(PlottedChannel { reference, colour, series: id });
    }

    /// Take a channel off this plot, by its position in the list.
    fn remove_channel(&mut self, position: usize) {
        if position >= self.channels.len() {
            return;
        }

        let plotted = self.channels.remove(position);
        if let Some(widget) = self.widget.as_mut() {
            let _ = widget.remove_series(&plotted.series);
        }

        // A plot of nothing is not a plot: back to the state a new one starts
        // in, rather than an empty set of axes.
        if self.channels.is_empty() {
            self.widget = None;
        }
    }

    /// A colour no trace on this plot is already using, where there is one.
    ///
    /// Going by the count alone would hand out a colour already on screen once
    /// a channel in the middle has been removed.
    fn next_colour(&self) -> Color {
        PALETTE
            .iter()
            .find(|colour| {
                !self.channels.iter().any(|plotted| plotted.colour == **colour)
            })
            .copied()
            .unwrap_or(PALETTE[self.channels.len() % PALETTE.len()])
    }
}

/// A line in the run log: what happened, and when.
struct LogEntry {
    at: DateTime<Utc>,
    text: String,
}

/// How many log lines to keep. A device that is failing every read would
/// otherwise fill memory with the same complaint.
const LOG_LIMIT: usize = 200;

// Fixed regions, not the freely-splittable tiling pane_grid also supports —
// resizable dividers only, no drag/split/close wired up.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PaneKind {
    Devices,
    Config,
    Data,
    /// Connections, errors, and anything else the run has to say. Separate
    /// from Config because none of it is a setting: it is what happened.
    Log,
}

struct AppDaq {
    devices: Vec<AppDevice>,
    plots: Vec<AppPlot>,
    /// Whether the settings dialog is up.
    settings_open: bool,
    /// What this person likes, as opposed to what this project is.
    ///
    /// Loaded at startup and written back as soon as anything changes: it is a
    /// tiny file and the changes are rare, so there is nothing here worth the
    /// debounce the project files need.
    settings: UserSettings,
    panes: pane_grid::State<PaneKind>,
    /// The project directory open at the moment, and None when none is.
    ///
    /// Everything that gets saved is saved into it, so with nothing open there
    /// is nothing to save to and the interface says so rather than writing
    /// somewhere arbitrary.
    project: Option<PathBuf>,
    /// Where each plot sits, by plot number. A grid of its own inside the
    /// Data pane, so plots can be arranged against each other without being
    /// draggable over the sidebar.
    plot_panes: pane_grid::State<usize>,
    /// The rig as configured. Kept so a run can be built again after being
    /// stopped: `Daq::from_config` consumes one, and the last one went with
    /// the thread that is now finishing.
    config: DaqConfig,
    /// The run in progress, or what is left of one that is stopping. None once
    /// a stopped run has been reaped.
    acquisition: Option<Acquisition>,
    run: RunState,
    /// Oldest first, read downwards like a terminal. The pane is anchored to
    /// its bottom, so the newest line is the one in view.
    log: Vec<LogEntry>,
    /// Whether the log pane is open, and where its divider was when it last
    /// was — so opening it again returns it to where it was dragged to rather
    /// than to a constant.
    log_open: bool,
    log_ratio: f32,
    /// How much of the recent past every plot shows, from the saved layout.
    window_seconds: f64,
    /// Every channel the setup measures, in the form a plot names one. What
    /// the settings panel offers when adding a channel to a plot.
    available: Vec<ChannelRef>,
    /// What the settings panel is showing.
    selected: Option<Selection>,
    /// The open right click menu, if one is.
    context: Option<ContextMenu>,
    /// Whether the file menu under the app name is open.
    file_menu: bool,
    /// Where the pointer was when the open menu was asked for.
    ///
    /// Taken once, at the click. Drawing at the live cursor instead would have
    /// the menu trail the pointer around the window.
    context_at: iced::Point,
    /// The serial ports found last time anybody looked.
    ///
    /// Held rather than asked for while drawing: listing them asks the
    /// operating system, and `view` runs every frame. Refreshed when a list is
    /// about to be useful — a serial device selected, or one being added.
    ports: Vec<serial_stream::PortOption>,
    /// The NI devices the driver reported last time anybody looked. Empty on a
    /// machine with no NI software, which is the ordinary case.
    ni_devices: Vec<String>,
    /// How many analog inputs the selected device has, where the hardware has
    /// said. None when it has not, which is why the lists below fall back to
    /// offering everything the backend allows rather than nothing at all.
    ni_inputs: Option<usize>,
    pico_inputs: Option<u16>,
    /// What the selected device turned out to be — `USB-6001`, `ADC-24` —
    /// where the hardware could say. Never written to the config: it describes
    /// what is plugged in now, not what the project is.
    model: Option<String>,
    /// The type chosen in the add-device dialog, and whether it is open at
    /// all. A device cannot be added without one, since the type decides what
    /// the rest of its settings even are.
    adding_device: Option<Option<&'static str>>,
    /// The number field of the selected channel, as it is being typed.
    ///
    /// Held rather than formatted afresh each time for two reasons. A
    /// `text_input` borrows its value for as long as the widget lives, so it
    /// cannot come from a local. And a number being typed passes through "",
    /// "1" and "1." on the way to "1.5": keeping the text means the field says
    /// what was typed while the config only takes it once it parses.
    number_draft: String,
    /// When the layout last changed, while it is still waiting to be written.
    /// None once it is saved.
    dirty_since: Option<Instant>,
    /// The same, for the rig's own configuration.
    rig_dirty_since: Option<Instant>,
    /// Where the plots' x axis has its zero.
    reference: DateTime<Utc>,
}

#[derive(Debug, Clone)]
enum Message {
    ThemeChanged(Theme),
    FontStepChanged(u8),
    LastProjectOpened,
    SettingsOpened,
    SettingsClosed,
    AddDeviceOpened,
    AddDeviceTypeChosen(&'static str),
    AddDeviceConfirmed,
    AddDeviceCancelled,
    DeviceDeleted(usize),
    ChannelDeleted(usize, usize),
    AddPlot,
    AllPlotsSelected,
    HistoryChanged(u64),
    PlotRenamed(usize, String),
    PlotDeleted(usize),
    ChannelAddedToPlot(usize, ChannelRef),
    ChannelRemovedFromPlot(usize, usize),
    ToggleDevice(usize),
    DeviceSelected(usize),
    ContextOpened(ContextMenu),
    ContextDismissed,
    FileMenuToggled,
    Exported,
    ProjectOpened,
    ProjectCreated,
    DeviceRenamed(usize, String),
    DeviceIntervalChanged(usize, u64),
    ChannelSelected(usize, usize),
    ChannelRenamed(usize, usize, String),
    ChannelUnitChanged(usize, usize, String),
    ScaleEdited(usize, usize, String),
    ChannelAdded(usize),
    SerialIndexEdited(usize, usize, String),
    PicoChannelChosen(usize, usize, u16),
    PicoRangeChosen(usize, usize, PicoRange),
    PicoSingleEnded(usize, usize, bool),
    NiChannelChosen(usize, usize, u32),
    NiRangeChosen(usize, usize, NiRange),
    NiSingleEnded(usize, usize, bool),
    PortsRefreshed,
    NiDevicesRefreshed,
    SerialPortChosen(usize, String),
    SerialBaudChosen(usize, u32),
    SerialPatternEdited(usize, String),
    NiDeviceEdited(usize, String),
    MockModeChanged(usize, MockMode),
    MockIntervalChanged(usize, u64),
    MockInputChanged(usize, usize, MockInputKind),
    MockNumberEdited(usize, usize, String),
    LogToggled,
    RunStarted,
    RunStopped,
    RecordPressed,
    PaneResized(pane_grid::ResizeEvent),
    PlotsResized(pane_grid::ResizeEvent),
    PlotClicked(pane_grid::Pane),
    PlotDragged(pane_grid::DragEvent),
    Plot(usize, PlotUiMessage),
    Tick,
}

impl AppDaq {
    fn new() -> Self {
        // Read before the interface exists, so the first frame is already
        // drawn the way this person asked for.
        let (settings, complaint) = UserSettings::load();

        // Nothing open yet. An empty rig is a perfectly good rig - no devices,
        // no plots - which is what lets the rest of the interface be written as
        // though a project were always there. The one thing genuinely missing
        // is a directory to save into, and that is what `project` being None
        // says.
        let mut app = AppDaq {
            project: None,
            devices: Vec::new(),
            plots: Vec::new(),
            settings,
            settings_open: false,
            panes: default_panes(),
            plot_panes: pane_grid::State::with_configuration(Configuration::Pane(1)),
            config: empty_config(),
            acquisition: None,
            run: RunState::Stopped,
            log: Vec::new(),
            log_open: false,
            log_ratio: LOG_OPEN,
            window_seconds: DEFAULT_HISTORY_SECONDS as f64,
            available: Vec::new(),
            selected: None,
            context: None,
            file_menu: false,
            context_at: iced::Point::ORIGIN,
            ports: serial_stream::available_ports(),
            // Not asked for at startup: loading the NI driver on a machine that
            // has one is slower than listing serial ports, and a project with
            // no NI device in it should never touch it.
            ni_devices: Vec::new(),
            ni_inputs: None,
            pico_inputs: None,
            model: None,
            adding_device: None,
            number_draft: String::new(),
            // Nothing to write: there is nothing open to have changed.
            dirty_since: None,
            rig_dirty_since: None,
            reference: Utc::now(),
        };

        // A settings file that would not read is worth saying out loud, and
        // this is the first moment there is a log to say it in.
        if let Some(problem) = complaint {
            app.note(problem);
        }

        // A project named on the command line opens straight away. Without one
        // the interface comes up asking for a folder, which is the only state
        // in which there is nothing to show.
        if let Some(named) = std::env::args().nth(1) {
            app.open_project(PathBuf::from(named));
        }
        app
    }

    /// Open a project directory, replacing whatever was open before.
    ///
    /// Everything belonging to the last project goes with it: its rig, its
    /// plots, their arrangement, and the run reading it. A config that will not
    /// read leaves the interface as it was and says so, rather than half
    /// opening onto a project that is not there.
    fn open_project(&mut self, directory: PathBuf) {
        let project = Project::new(&directory);

        let config = match project.read_config() {
            Ok(config) => config,
            Err(error) => {
                self.note(format!("could not open {}: {}", directory.display(), error));
                return;
            }
        };

        self.stop_acquisition();

        let known = config.available_inputs();
        let mut notes: Vec<String> = Vec::new();

        let (window_seconds, plots, saved_layout) = match project.read_layout() {
            Ok(Some(saved)) => {
                let window = saved.history_seconds as f64;

                // Asked once, of the library, rather than worked out per plot
                // here: whether a layout and a rig agree is the same question
                // however it is being looked at.
                for reference in saved.dangling(&config) {
                    notes.push(format!(
                        "{} names {}, which this setup does not have",
                        plot_config::FILE,
                        reference
                    ));
                }

                let plots = saved
                    .plots
                    .iter()
                    .map(|plot| {
                        // Skipped rather than refused: a plot pointing at
                        // something that is gone should not stop the rest being
                        // drawn, which is how choptui treats it too.
                        let drawable: Vec<ChannelRef> = plot
                            .channels
                            .iter()
                            .filter(|reference| known.contains(reference))
                            .cloned()
                            .collect();

                        build_plot(plot.number, plot.display_name(), drawable, window)
                    })
                    .collect();

                notes.push(format!("layout read from {}", plot_config::FILE));
                (window, plots, saved.layout)
            }
            Ok(None) => {
                // No saved layout: everything measured on one plot, which is
                // at least a view of the whole rig to start from.
                notes.push(format!("no {}, showing every channel on one plot", plot_config::FILE));
                let window = DEFAULT_HISTORY_SECONDS as f64;
                (window, vec![build_plot(1, "Plot 1".to_string(), known.clone(), window)], None)
            }
            Err(problem) => {
                notes.push(problem.to_string());
                let window = DEFAULT_HISTORY_SECONDS as f64;
                (window, vec![build_plot(1, "Plot 1".to_string(), known.clone(), window)], None)
            }
        };

        // The saved arrangement where there is one, pruned to the plots that
        // exist, with anything it does not mention stacked on afterwards. A
        // plot added from the terminal has never been arranged, and should
        // still appear.
        let numbers: Vec<usize> = plots.iter().map(|plot| plot.number).collect();
        let arranged = saved_layout
            .as_ref()
            .and_then(|layout| configuration_from(layout, &numbers))
            .unwrap_or_else(|| {
                stacked(&numbers).unwrap_or(Configuration::Pane(numbers.first().copied().unwrap_or(1)))
            });

        let mut plot_panes = pane_grid::State::with_configuration(arranged);
        let placed: Vec<usize> = plot_panes.iter().map(|(_, number)| *number).collect();
        for number in numbers.iter().filter(|number| !placed.contains(number)) {
            let last = plot_panes.iter().map(|(pane, _)| *pane).last();
            if let Some(last) = last {
                plot_panes.split(Axis::Horizontal, last, *number);
            }
        }

        self.devices = devices_from(&config);
        self.available = known;
        self.plots = plots;
        self.plot_panes = plot_panes;
        self.window_seconds = window_seconds;
        self.selected = None;
        self.context = None;
        self.reference = Utc::now();
        // What is on screen is what the files say, so there is nothing owed to
        // disk for the project just opened.
        self.dirty_since = None;
        self.rig_dirty_since = None;
        self.project = Some(directory.clone());
        self.settings.set_last_project(&directory);
        self.remember_settings();

        // Opened stopped, not reading. A rig is nearly always opened to be
        // looked at or changed, and configuration is locked while a run is on,
        // so starting one for you gets in the way of the thing you came to do.
        // Play is one press away when reading is what you wanted.
        self.config = config;
        self.set_plot_panning(true);
        // Once, so the axes open on a sensible range rather than on the seed
        // point each series is built with.
        self.slide_viewport();

        self.note(format!("project {}", directory.display()));
        for note in notes {
            self.note(note);
        }
    }

    /// Begin reading the project that is open.
    ///
    /// Nothing to read without one: a run records beside its project, so there
    /// is no sensible place for one to happen with nothing open.
    fn start_run(&mut self) {
        let Some(directory) = self.project.clone() else {
            self.note("no project open to read".to_string());
            return;
        };

        self.acquisition = Some(start_acquisition(self.config.clone(), directory));
        self.run = RunState::Running;
        self.set_plot_panning(false);
        self.note("acquisition started".to_string());
    }

    /// What the interface shows before a project has been chosen.
    ///
    /// The same two actions the file menu offers, put where they cannot be
    /// missed: with nothing open, finding the menu is the only thing there is
    /// to do, so the dialog does it instead.
    fn welcome(&self) -> Element<'_, Message> {
        container(
            column![
                text("No project open").size(20),
                text(
                    "A project is a folder holding config.json, which describes the rig, \
                     alongside its layout and its recordings."
                )
                .size(13),
                row![
                    button(text("Open a project").size(14))
                        .padding(8)
                        .on_press(Message::ProjectOpened),
                    button(text("New project").size(14))
                        .style(button::secondary)
                        .padding(8)
                        .on_press(Message::ProjectCreated),
                ]
                .spacing(8),
            ]
            .spacing(12)
            // The last project, when there still is one. Checked rather than
            // trusted: a folder that has been moved, renamed or emptied since
            // is not worth offering, and finding that out by failing to open
            // it would be a worse way to say so.
            .extend(self.reopenable().map(|(directory, name)| {
                column![
                    text("Last opened").size(12),
                    button(text(name).size(14))
                        .style(button::success)
                        .padding(8)
                        .on_press(Message::LastProjectOpened),
                    text(directory).size(11),
                ]
                .spacing(4)
                .into()
            }))
            // `extend` over an Option, because an Option is an iterator of at
            // most one thing and iced 0.14's Column has no `push_maybe`.
            .extend(self.log.last().map(|entry| {
                // The log pane is behind this layer and cannot be read, so a
                // refused folder would otherwise report into somewhere
                // invisible. Only the last line, because the only thing worth
                // saying here is why the last attempt did not take.
                text(entry.text.clone())
                    .size(12)
                    .style(|theme: &Theme| text::Style {
                        color: Some(theme.extended_palette().danger.base.color),
                    })
                    .into()
            }))
            .width(420),
        )
        .padding(20)
        .style(dialog_style)
        .into()
    }

    /// The last project, if it is still one, as its name and its path.
    ///
    /// The name alone is what somebody recognises; the path is there because
    /// two rigs called "test" in different places are not unusual.
    fn reopenable(&self) -> Option<(String, String)> {
        let directory = self.settings.last_project.as_ref()?;
        if !Project::new(directory).config_path().exists() {
            return None;
        }

        let name = directory
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| directory.display().to_string());

        Some((directory.display().to_string(), name))
    }

    /// Where a folder picker should start.
    ///
    /// Beside whatever is open, since the next project is usually a sibling of
    /// the last one. The working directory otherwise, which is where a project
    /// made from the command line would be.
    fn pick_from(&self) -> PathBuf {
        self.project
            .as_ref()
            .and_then(|open| open.parent().map(Path::to_path_buf))
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."))
    }

    /// The project open at the moment, where one is.
    fn project(&self) -> Option<Project> {
        self.project.as_ref().map(Project::new)
    }

    /// End the run reading the project being closed.
    ///
    /// Asked to stop rather than waited for: the thread holds only its own
    /// devices and a channel nobody will read again, and blocking until it
    /// noticed would freeze the window in the middle of opening something.
    fn stop_acquisition(&mut self) {
        if let Some(acquisition) = self.acquisition.take() {
            acquisition.recording.store(false, Ordering::Relaxed);
            acquisition.stop.store(true, Ordering::Relaxed);
        }
        self.run = RunState::Stopped;
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::ThemeChanged(theme) => {
                self.settings.set_theme(&theme);
                self.remember_settings();
            }
            Message::FontStepChanged(step) => {
                self.settings.font_step = step;
                self.remember_settings();
            }
            Message::LastProjectOpened => {
                if let Some(directory) = self.settings.last_project.clone() {
                    self.open_project(directory);
                }
            }
            Message::SettingsOpened => {
                self.file_menu = false;
                self.settings_open = true;
            }
            Message::SettingsClosed => self.settings_open = false,
            Message::AddDeviceOpened => {
                // Looked up now rather than while drawing, and now is when it
                // matters: something may have been plugged in since startup.
                self.ports = serial_stream::available_ports();
                self.ni_devices = lumberdaq::hardware::ni_daqmx::available_devices();
                self.adding_device = Some(None);
            }
            Message::AddDeviceCancelled => self.adding_device = None,
            Message::AddDeviceTypeChosen(kind) => self.adding_device = Some(Some(kind)),
            Message::AddDeviceConfirmed => {
                let Some(Some(kind)) = self.adding_device else { return };
                let Some(mut hardware) = HardwareConfig::of_type(kind) else { return };

                // A serial device starts on the first USB port there is, which
                // is nearly always the instrument somebody just plugged in.
                // The backend leaves the port empty rather than guessing; this
                // is the interface making the guess, where it can be seen and
                // changed in the dropdown right next to it.
                if let HardwareConfig::SerialStream(serial) = &mut hardware {
                    if let Some(port) = self.ports.iter().find(|port| port.usb) {
                        serial.port = port.name.clone();
                    }
                }

                // Named for what it is until somebody says otherwise, and made
                // unique so two new devices are not both "New device" — the
                // name is how a batch finds its way back to a channel.
                let name = unused_name("New device", &self.config);

                self.config.devices.push(lumberdaq::config::DeviceConfig {
                    info: lumberdaq::device::DeviceInfo { name: name.clone() },
                    read_interval_ms: lumberdaq::config::default_read_interval_ms(),
                    hardware,
                });
                self.devices.push(AppDevice { name, channels: Vec::new(), expanded: false });

                self.adding_device = None;
                // Selected so its settings are there to fill in: a device with
                // no channels is not finished being made.
                self.selected = Some(Selection::Device(self.config.devices.len() - 1));
                self.rig_changed();
            }
            Message::DeviceDeleted(index) => {
                self.context = None;
                if index < self.config.devices.len() {
                    let device = self.config.devices.remove(index);
                    self.devices.remove(index);
                    // Its channels cannot be plotted any more, so they come off
                    // the plots rather than being left as traces that will
                    // never receive another point.
                    self.forget_device_on_plots(&device.info.name);

                    self.note(format!("deleted {}", device.info.name));
                    self.selected = None;
                    self.rig_changed();
                }
            }
            Message::ChannelDeleted(device, channel) => {
                self.context = None;
                let Some(device_config) = self.config.devices.get_mut(device) else { return };
                let device_name = device_config.info.name.clone();
                let Some(info) = device_config.hardware.channel_info(channel).cloned() else {
                    return;
                };

                if device_config.hardware.remove_channel(channel) {
                    if let Some(app_device) = self.devices.get_mut(device) {
                        app_device.channels.remove(channel);
                    }
                    self.forget_channel_on_plots(&ChannelRef {
                        device: device_name,
                        channel: info.name.clone(),
                    });

                    self.note(format!("deleted {}", info.name));
                    self.selected = Some(Selection::Device(device));
                    self.rig_changed();
                }
            }
            Message::PlotClicked(pane) => {
                self.context = None;
                // The pane knows its plot's number; the settings panel works
                // in positions, so it is looked up rather than assumed equal.
                let number = self.plot_panes.get(pane).copied();
                if let Some(index) =
                    number.and_then(|number| {
                        self.plots.iter().position(|plot| plot.number == number)
                    })
                {
                    self.selected = Some(Selection::Plot(index));
                }
            }
            Message::PlotDragged(pane_grid::DragEvent::Dropped { pane, target }) => {
                self.plot_panes.drop(pane, target);
                self.layout_changed();
            }
            // Picked up and put back, or let go somewhere that is not a pane.
            // Nothing has moved, so there is nothing to save.
            Message::PlotDragged(_) => {}
            Message::PlotsResized(pane_grid::ResizeEvent { split, ratio }) => {
                self.plot_panes.resize(split, ratio);
                // The arrangement is part of the layout, so dragging a divider
                // is a change to be saved like renaming a plot is.
                self.layout_changed();
            }
            Message::AddPlot => {
                // Past the highest in use rather than the length, so deleting
                // plot 2 of three and adding one does not reuse the number.
                let number =
                    self.plots.iter().map(|plot| plot.number).max().unwrap_or(0) + 1;
                let name = format!("Plot {}", number);
                self.plots.push(build_plot(number, name, Vec::new(), self.window_seconds));

                // Split whatever is last rather than rebuilding the grid, so
                // the arrangement already made survives adding to it.
                match self.plot_panes.iter().map(|(pane, _)| *pane).last() {
                    Some(last) => {
                        self.plot_panes.split(Axis::Horizontal, last, number);
                    }
                    None => self.plot_panes = pane_grid::State::with_configuration(
                        Configuration::Pane(number),
                    ),
                }
                // A new widget comes with the default bindings, which is the
                // wrong answer mid-run.
                let stopped = self.run == RunState::Stopped;
                self.set_plot_panning(stopped);

                // Selected so it can be configured: an empty plot is no use
                // until something is put on it.
                self.selected = Some(Selection::Plot(self.plots.len() - 1));
                self.layout_changed();
            }
            Message::AllPlotsSelected => {
                self.selected = Some(Selection::AllPlots);
            }
            Message::HistoryChanged(seconds) => {
                // Taken up by the viewport on the next frame and by the trim
                // on the next batch, so there is nothing to apply to each plot
                // here. A longer window does not bring back points already
                // trimmed: the traces grow into it as data arrives.
                self.window_seconds = seconds as f64;
                // Applied here as well as by the frame tick, because stopped
                // there is no frame tick to apply it and the setting would
                // look broken until the next run.
                self.slide_viewport();
                self.layout_changed();
            }
            Message::PlotRenamed(index, name) => {
                if let Some(plot) = self.plots.get_mut(index) {
                    plot.name = name;
                    self.layout_changed();
                }
            }
            Message::ChannelAddedToPlot(index, reference) => {
                let window = self.window_seconds;
                if let Some(plot) = self.plots.get_mut(index) {
                    plot.add_channel(reference, window);
                    // The first channel on a plot builds its widget, which
                    // arrives with the default bindings.
                    let stopped = self.run == RunState::Stopped;
                    self.set_plot_panning(stopped);
                    self.layout_changed();
                }
            }
            Message::ChannelRemovedFromPlot(index, position) => {
                if let Some(plot) = self.plots.get_mut(index) {
                    plot.remove_channel(position);
                    self.layout_changed();
                }
            }
            Message::PlotDeleted(index) => {
                self.context = None;
                if index < self.plots.len() {
                    let plot = self.plots.remove(index);

                    // Closing its pane hands the space back to its neighbour,
                    // which is what makes the arrangement close up rather than
                    // leave a gap where the plot was.
                    // Found into a binding first: on this edition the borrow in
                    // an `if let` scrutinee lasts for the whole block, so
                    // closing inside one would still be holding the read.
                    let pane = self
                        .plot_panes
                        .iter()
                        .find(|(_, number)| **number == plot.number)
                        .map(|(pane, _)| *pane);

                    if let Some(pane) = pane {
                        self.plot_panes.close(pane);
                    }

                    self.note(format!("deleted {}", plot.name));
                    // The panel was showing a plot that no longer exists, and
                    // every index after this one has moved.
                    self.selected = None;
                    self.layout_changed();
                }
            }
            Message::ToggleDevice(index) => {
                if let Some(device) = self.devices.get_mut(index) {
                    device.expanded = !device.expanded;
                }
            }
            // Any press puts an open menu away. It fires alongside whatever
            // was actually clicked rather than instead of it, which is what
            // makes clicking straight through to something else work: the menu
            // closes and the thing under the cursor still gets its click.
            Message::ContextDismissed => {
                self.context = None;
                self.file_menu = false;
            }
            Message::FileMenuToggled => self.file_menu = !self.file_menu,
            Message::ProjectOpened => {
                self.file_menu = false;
                // Blocking, and deliberately: the picker is modal, so the
                // window behind it is meant to be waiting. Doing it without an
                // async runtime is worth more here than not blocking during a
                // dialog nobody can see past.
                if let Some(directory) = rfd::FileDialog::new()
                    .set_title("Open project")
                    .set_directory(self.pick_from())
                    .pick_folder()
                {
                    self.open_project(directory);
                }
            }
            Message::ProjectCreated => {
                self.file_menu = false;
                let Some(directory) = rfd::FileDialog::new()
                    .set_title("New project: choose an empty folder")
                    .set_directory(self.pick_from())
                    .pick_folder()
                else {
                    return;
                };

                // Refused rather than merged: opening a folder that is already
                // a project is what Load is for, and writing a fresh config
                // over one would throw a rig away.
                let project = Project::new(&directory);
                if project.config_path().exists() {
                    self.note(format!(
                        "{} is already a project - open it rather than creating one",
                        directory.display()
                    ));
                    return;
                }

                let named = directory
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_default();

                let fresh = DaqConfig { info: DaqInfo { name: named, ..empty_config().info }, ..empty_config() };
                match lumberdaq::configuration::write_configuration_file(
                    &project.config_path(),
                    &fresh,
                ) {
                    Ok(()) => {
                        self.note(format!("created {}", project.config_path().display()));
                        self.open_project(directory);
                    }
                    Err(error) => self.note(format!("could not create the project: {}", error)),
                }
            }
            Message::Exported => {
                self.file_menu = false;

                let Some(project) = self.project() else {
                    self.note("no project open to export".to_string());
                    return;
                };

                // Read only, and skipping any run already written, so pressing
                // this twice costs nothing and cannot touch the results.
                match lumberdaq::export::export(
                    &project.database_path(),
                    &project.export_path(),
                ) {
                    Ok(report) => self.note(match report.written.len() {
                        0 => "nothing new to export".to_string(),
                        written => format!(
                            "exported {} run(s) to {}",
                            written,
                            project.export_path().display()
                        ),
                    }),
                    Err(error) => self.note(format!("could not export: {}", error)),
                }
            }
            Message::ContextOpened(target) => {
                // Right clicking the same thing again puts the menu away,
                // which is the way out of one without choosing from it.
                self.context = match self.context == Some(target) {
                    true => None,
                    false => {
                        self.context_at = pointer();
                        Some(target)
                    }
                };
            }
            Message::DeviceSelected(index) => {
                // Refreshed on selecting a serial device, since its port list
                // is about to be shown and something may have been unplugged.
                self.context = None;
                // Asked once, on selection, rather than while drawing: every
                // one of these talks to a driver.
                self.model = None;
                match self.config.devices.get(index).map(|device| &device.hardware) {
                    Some(HardwareConfig::SerialStream(serial)) => {
                        self.ports = serial_stream::available_ports();
                        // Whatever the port says it is, which is as close to a
                        // model as a serial device gets: the thing on the other
                        // end is only a stream of bytes.
                        self.model = self
                            .ports
                            .iter()
                            .find(|port| port.name == serial.port)
                            .map(|port| port.product.clone())
                            .filter(|product| !product.is_empty());
                    }
                    Some(HardwareConfig::NiDaqmx(ni)) => {
                        self.ni_devices = lumberdaq::hardware::ni_daqmx::available_devices();
                        // How many inputs it has decides both which channels
                        // can be chosen and which one a differential pair
                        // takes, and only the device knows.
                        self.ni_inputs = lumberdaq::hardware::ni_daqmx::input_count(&ni.device);
                        self.model = lumberdaq::hardware::ni_daqmx::product_type(&ni.device);
                    }
                    Some(HardwareConfig::PicoHrdl(_)) => {
                        // An ADC-20 has eight inputs where an ADC-24 has
                        // sixteen. Asked only while stopped, since a unit being
                        // read cannot also be opened to ask.
                        if self.run == RunState::Stopped {
                            self.pico_inputs = pico_hrdl::input_count();
                            self.model = pico_hrdl::variant();
                        } else {
                            self.pico_inputs = None;
                        }
                    }
                    _ => {}
                }
                self.selected = Some(Selection::Device(index));
            }
            Message::ChannelSelected(device, channel) => {
                self.context = None;
                self.selected = Some(Selection::Channel(device, channel));
                self.number_draft = self.channel_number(device, channel);
            }
            Message::ChannelAdded(index) => {
                self.context = None;
                let Some(device) = self.config.devices.get_mut(index) else { return };

                // Named so it is findable, and uniquely, since a name is how a
                // reading gets back to the channel it came from.
                let existing: Vec<String> = device
                    .hardware
                    .channel_infos()
                    .into_iter()
                    .map(|info| info.name)
                    .collect();
                let name = unused_channel_name("New channel", &existing);

                let info = lumberdaq::channel::ChannelInfo {
                    name: name.clone(),
                    ..Default::default()
                };

                if device.hardware.add_channel(info) {
                    let position = device.hardware.channel_infos().len() - 1;
                    // Read back rather than assumed: the backend may have
                    // filled in the unit its readings arrive in.
                    let unit = device
                        .hardware
                        .channel_info(position)
                        .map(|info| info.unit.clone())
                        .unwrap_or_default();

                    if let Some(app_device) = self.devices.get_mut(index) {
                        app_device.expanded = true;
                        app_device.channels.push(AppChannel {
                            name,
                            unit,
                            latest: None,
                            samples: 0,
                        });
                    }
                    // Straight to its settings: a channel bound to whatever
                    // input came next is a starting point, not an answer.
                    self.selected = Some(Selection::Channel(index, position));
                    self.number_draft = self.channel_number(index, position);
                    self.rig_changed();
                }
            }
            Message::SerialIndexEdited(device, channel, text) => {
                // Whole fields only, and not negative: an index counts fields
                // of a frame. The config hears about it once it is one, while
                // the field shows what is being typed either way.
                if let Ok(index) = text.parse::<u32>() {
                    if let Some(HardwareConfig::SerialStream(serial)) =
                        self.config.devices.get_mut(device).map(|device| &mut device.hardware)
                    {
                        if let Some(channel) = serial.channels.get_mut(channel) {
                            channel.index = index as i64;
                            self.rig_changed();
                        }
                    }
                }
                self.number_draft = text;
            }
            Message::PicoChannelChosen(device, channel, number) => {
                if let Some(HardwareConfig::PicoHrdl(pico)) =
                    self.config.devices.get_mut(device).map(|device| &mut device.hardware)
                {
                    if let Some(channel) = pico.channels.get_mut(channel) {
                        channel.channel = number;
                        self.rig_changed();
                    }
                }
            }
            Message::PicoRangeChosen(device, channel, range) => {
                if let Some(HardwareConfig::PicoHrdl(pico)) =
                    self.config.devices.get_mut(device).map(|device| &mut device.hardware)
                {
                    if let Some(channel) = pico.channels.get_mut(channel) {
                        channel.range = range.0;
                        self.rig_changed();
                    }
                }
            }
            Message::PicoSingleEnded(device, channel, single_ended) => {
                if let Some(HardwareConfig::PicoHrdl(pico)) =
                    self.config.devices.get_mut(device).map(|device| &mut device.hardware)
                {
                    if let Some(channel) = pico.channels.get_mut(channel) {
                        channel.single_ended = single_ended;
                        self.rig_changed();
                    }
                }
            }
            Message::NiChannelChosen(device, channel, number) => {
                if let Some(HardwareConfig::NiDaqmx(ni)) =
                    self.config.devices.get_mut(device).map(|device| &mut device.hardware)
                {
                    if let Some(channel) = ni.channels.get_mut(channel) {
                        channel.channel = number;
                        self.rig_changed();
                    }
                }
            }
            Message::NiRangeChosen(device, channel, range) => {
                if let Some(HardwareConfig::NiDaqmx(ni)) =
                    self.config.devices.get_mut(device).map(|device| &mut device.hardware)
                {
                    if let Some(channel) = ni.channels.get_mut(channel) {
                        channel.range = range.0;
                        self.rig_changed();
                    }
                }
            }
            Message::NiSingleEnded(device, channel, single_ended) => {
                if let Some(HardwareConfig::NiDaqmx(ni)) =
                    self.config.devices.get_mut(device).map(|device| &mut device.hardware)
                {
                    if let Some(channel) = ni.channels.get_mut(channel) {
                        channel.single_ended = single_ended;
                        self.rig_changed();
                    }
                }
            }
            Message::PortsRefreshed => {
                self.ports = serial_stream::available_ports();
                self.note(format!("{} serial port(s) found", self.ports.len()));
            }
            Message::NiDevicesRefreshed => {
                self.ni_devices = lumberdaq::hardware::ni_daqmx::available_devices();
                self.note(format!("{} NI device(s) found", self.ni_devices.len()));
            }
            Message::SerialPortChosen(index, port) => {
                if let Some(HardwareConfig::SerialStream(serial)) =
                    self.config.devices.get_mut(index).map(|device| &mut device.hardware)
                {
                    serial.port = port;
                    self.rig_changed();
                }
            }
            Message::SerialBaudChosen(index, baudrate) => {
                if let Some(HardwareConfig::SerialStream(serial)) =
                    self.config.devices.get_mut(index).map(|device| &mut device.hardware)
                {
                    serial.baudrate = baudrate;
                    self.rig_changed();
                }
            }
            Message::SerialPatternEdited(index, pattern) => {
                if let Some(HardwareConfig::SerialStream(serial)) =
                    self.config.devices.get_mut(index).map(|device| &mut device.hardware)
                {
                    serial.frame_pattern = pattern;
                    self.rig_changed();
                }
            }
            Message::NiDeviceEdited(index, name) => {
                if let Some(HardwareConfig::NiDaqmx(ni)) =
                    self.config.devices.get_mut(index).map(|device| &mut device.hardware)
                {
                    ni.device = name;
                    self.rig_changed();
                }
            }
            Message::MockModeChanged(index, mode) => {
                if let Some(HardwareConfig::MockHardware(mock)) =
                    self.config.devices.get_mut(index).map(|device| &mut device.hardware)
                {
                    mock.acquisition = match mode {
                        MockMode::Polled => mock_hardware::Acquisition::Polled,
                        // The interval it had, or a default it can be changed
                        // from: switching mode should not also be choosing a
                        // rate somebody never said.
                        MockMode::Streaming => mock_hardware::Acquisition::Streaming {
                            sample_interval_ms: match mock.acquisition {
                                mock_hardware::Acquisition::Streaming { sample_interval_ms } => sample_interval_ms,
                                mock_hardware::Acquisition::Polled => 100,
                            },
                        },
                    };
                    self.rig_changed();
                }
            }
            Message::MockIntervalChanged(index, interval) => {
                if let Some(HardwareConfig::MockHardware(mock)) =
                    self.config.devices.get_mut(index).map(|device| &mut device.hardware)
                {
                    mock.acquisition = mock_hardware::Acquisition::Streaming { sample_interval_ms: interval };
                    self.rig_changed();
                }
            }
            Message::MockInputChanged(device, channel, kind) => {
                if let Some(input) = self.mock_input_mut(device, channel) {
                    // The number carried over where the new kind has one, so
                    // flipping between Constant and Sine to look at them does
                    // not quietly lose what was typed.
                    let number = MockInputKind::number(input).unwrap_or(1.0);
                    *input = match kind {
                        MockInputKind::Random => MockHardwareInput::Random,
                        MockInputKind::Constant => MockHardwareInput::Constant(number),
                        MockInputKind::Sine => MockHardwareInput::Sine { frequency_hz: number },
                    };
                    self.number_draft = self.channel_number(device, channel);
                    self.rig_changed();
                }
            }
            Message::MockNumberEdited(device, channel, text) => {
                // The field shows what was typed either way; the config only
                // hears about it once it is a number.
                if let Ok(number) = text.parse::<f64>() {
                    if let Some(input) = self.mock_input_mut(device, channel) {
                        *input = match input {
                            MockHardwareInput::Constant(_) => MockHardwareInput::Constant(number),
                            MockHardwareInput::Sine { .. } => {
                                MockHardwareInput::Sine { frequency_hz: number }
                            }
                            MockHardwareInput::Random => MockHardwareInput::Random,
                        };
                        self.rig_changed();
                    }
                }
                self.number_draft = text;
            }
            Message::DeviceRenamed(index, name) => {
                let Some(device) = self.config.devices.get_mut(index) else { return };
                let was = std::mem::replace(&mut device.info.name, name.clone());

                // The list on screen mirrors the config rather than holding its
                // own truth, so it is brought along rather than left to drift.
                if let Some(device) = self.devices.get_mut(index) {
                    device.name = name.clone();
                }
                // A plot names a channel by its device and its own name, so a
                // rename would otherwise leave every plot pointing at a device
                // that no longer exists — a trace that quietly stops receiving
                // anything, and a layout naming something that is not there.
                self.rename_device_on_plots(&was, &name);
                self.rig_changed();
            }
            Message::DeviceIntervalChanged(index, interval) => {
                if let Some(device) = self.config.devices.get_mut(index) {
                    device.read_interval_ms = interval;
                    self.rig_changed();
                }
            }
            Message::ChannelRenamed(device, channel, name) => {
                let Some(device_config) = self.config.devices.get(device) else { return };
                let device_name = device_config.info.name.clone();

                let Some(info) = self.channel_info_mut(device, channel) else { return };
                let was = std::mem::replace(&mut info.name, name.clone());

                if let Some(app_channel) =
                    self.devices.get_mut(device).and_then(|d| d.channels.get_mut(channel))
                {
                    app_channel.name = name.clone();
                }
                self.rename_channel_on_plots(&device_name, &was, &name);
                self.rig_changed();
            }
            Message::ChannelUnitChanged(device, channel, unit) => {
                if let Some(info) = self.channel_info_mut(device, channel) {
                    info.unit = unit.clone();
                }
                if let Some(app_channel) =
                    self.devices.get_mut(device).and_then(|d| d.channels.get_mut(channel))
                {
                    app_channel.unit = unit;
                }
                self.rig_changed();
            }
            Message::ScaleEdited(device, channel, equation) => {
                if let Some(info) = self.channel_info_mut(device, channel) {
                    info.scale = match equation.trim().is_empty() {
                        // An empty formula is no calculation, which is how the
                        // config says it too: the field is simply absent. So
                        // clearing the box is how one is removed, and there is
                        // no separate add or delete to find.
                        true => None,
                        // The parameters of a parameterised scale are kept:
                        // only the equation is being typed, and dropping the
                        // constants it refers to would break it on the first
                        // keystroke.
                        false => Some(match info.scale.take() {
                            Some(Scale::Parameterised { from, parameters, .. }) => {
                                Scale::Parameterised { from, equation, parameters }
                            }
                            _ => Scale::Equation(equation),
                        }),
                    };
                    self.rig_changed();
                }
            }
            Message::LogToggled => {
                // The log is the outermost split, so the root of the layout is
                // its divider. Resizing that is what opens and shuts it: a
                // pane_grid pane has no hidden state of its own.
                let Some((split, ratio)) = self.log_split() else { return };

                if self.log_open {
                    // Remembered before shutting, so a divider that was dragged
                    // somewhere deliberate returns there.
                    self.log_ratio = ratio;
                    self.panes.resize(split, LOG_SHUT);
                } else {
                    self.panes.resize(split, self.log_ratio);
                }
                self.log_open = !self.log_open;
            }
            Message::RunStarted => {
                if self.run == RunState::Stopped {
                    self.start_run();
                }
            }
            Message::RecordPressed => {
                // One button for both halves of the same intention. Recording
                // implies acquiring, so asking to record a stopped rig starts
                // it: nobody presses record meaning "write down nothing".
                if self.recording() {
                    if let Some(acquisition) = self.acquisition.as_ref() {
                        acquisition.recording.store(false, Ordering::Relaxed);
                    }
                    self.note("recording stopped".to_string());
                    return;
                }

                if self.run == RunState::Stopped {
                    self.start_run();
                }

                // Not while a previous run is still winding up: the flag would
                // be set on an acquisition that is about to be dropped.
                if self.run == RunState::Running {
                    if let Some(acquisition) = self.acquisition.as_ref() {
                        acquisition.recording.store(true, Ordering::Relaxed);
                        self.note("recording started".to_string());
                    }
                }
            }
            Message::RunStopped => {
                if self.run == RunState::Running {
                    if let Some(acquisition) = self.acquisition.as_ref() {
                        // Lowered before the run ends so the recorder flushes
                        // and closes its sink, rather than the recording being
                        // cut off wherever the last batch happened to land.
                        // It also means starting again does not silently
                        // resume recording.
                        acquisition.recording.store(false, Ordering::Relaxed);
                        acquisition.stop.store(true, Ordering::Relaxed);
                    }
                    // Not Stopped yet: the thread is still reading whatever it
                    // was in the middle of. `reap_stopped_run` decides when.
                    self.run = RunState::Stopping;
                    self.note("stopping".to_string());
                }
            }
            Message::PaneResized(pane_grid::ResizeEvent { split, ratio }) => {
                self.panes.resize(split, ratio);
            }
            Message::Plot(index, plot_message) => {
                if let Some(widget) = self.plots.get_mut(index).and_then(|plot| plot.widget.as_mut())
                {
                    widget.update(plot_message);
                }
            }
            Message::Tick => {
                // Take everything waiting, rather than one per frame: batches
                // arrive at each device's read interval and carry however many
                // samples were collected in it, so the count per frame varies.
                // Drained into a batch first: `record` and `note` both want
                // &mut self, which the receiver borrow would otherwise hold.
                let mut arrived = Vec::new();
                if let Some(acquisition) = self.acquisition.as_ref() {
                    while let Ok(message) = acquisition.from_acquisition.try_recv() {
                        arrived.push(message);
                    }
                }

                for message in arrived {
                    match message {
                        FromAcquisition::Data { device, channel, datapoints } => {
                            self.record(&device, &channel, &datapoints);
                        }
                        FromAcquisition::Status(text) => self.note(text),
                    }
                }

                self.reap_stopped_run();

                // Slid every frame whether or not anything arrived, which is
                // what keeps the scroll smooth while the data stays honest.
                // Only while reading: a stopped plot that kept sliding would
                // walk away from its own data, and would undo a pan on the
                // very next frame.
                if self.run != RunState::Stopped {
                    self.slide_viewport();
                }

                // The frame tick doubles as the timer the debounce needs, so
                // saving costs no extra machinery.
                self.save_layout_if_settled();
                self.save_rig_if_settled();
            }
        }
    }

    /// Put every plot's viewport on the last `window_seconds` up to now.
    ///
    /// Called every frame while reading, which is what makes the scroll
    /// smooth, and once by hand at the points where the span changes without a
    /// run to carry it through.
    fn slide_viewport(&mut self) {
        let now = seconds_since(self.reference, Utc::now());
        let window = self.window_seconds;
        for plot in self.plots.iter_mut() {
            if let Some(widget) = plot.widget.as_mut() {
                widget.set_x_lim(now - window, now);
            }
        }
    }

    /// Let the plots be panned by hand, or not.
    ///
    /// While a run is going the viewport is set from the clock on every frame,
    /// so a pan is overwritten the moment it is made — the trace springs back
    /// to now and the drag looks broken rather than disallowed. Unbinding the
    /// gesture says plainly that it is not available, and the cursor stops
    /// offering it. Box zoom on the right button is left alone: it sets a
    /// range rather than fighting for the same one.
    fn set_plot_panning(&mut self, allowed: bool) {
        for plot in self.plots.iter_mut() {
            let Some(widget) = plot.widget.as_mut() else { continue };
            let controls = widget.get_controls_mut();

            match allowed {
                true => {
                    controls.bind_drag(iced::mouse::Button::Left, iced_plot::DragAction::Pan);
                    controls.bind_scroll(
                        iced::keyboard::Modifiers::NONE,
                        iced_plot::ScrollAction::Pan,
                    );
                }
                false => {
                    controls.unbind_drag(iced::mouse::Button::Left);
                    controls.unbind_scroll(iced::keyboard::Modifiers::NONE);
                }
            }
        }
    }

    /// Whether a recording is being asked for.
    ///
    /// The flag rather than the `Recorder` itself, which is on the other side
    /// of the thread. It is what was asked for; the recorder acts on it when
    /// the next batch or flush comes round.
    fn recording(&self) -> bool {
        self.acquisition
            .as_ref()
            .is_some_and(|acquisition| acquisition.recording.load(Ordering::Relaxed))
    }

    /// Let go of a run that has finished stopping.
    ///
    /// `is_finished` rather than joining outright: `join` would block the
    /// interface until the thread came round its read loop, which is up to a
    /// whole read interval of frozen window. Asking every frame costs nothing
    /// and the join is instant once it answers yes.
    fn reap_stopped_run(&mut self) {
        if self.run != RunState::Stopping {
            return;
        }

        let finished = self
            .acquisition
            .as_ref()
            .is_some_and(|acquisition| acquisition.thread.is_finished());

        if finished {
            if let Some(acquisition) = self.acquisition.take() {
                // Instant, now that the thread has ended, and it is what turns
                // a panic in there into something we hear about.
                if acquisition.thread.join().is_err() {
                    self.note("the acquisition thread panicked".to_string());
                }
            }
            self.run = RunState::Stopped;
            self.set_plot_panning(true);
            self.note("acquisition stopped".to_string());
        }
    }

    /// One channel's description in the configuration, to change.
    fn channel_info_mut(
        &mut self,
        device: usize,
        channel: usize,
    ) -> Option<&mut lumberdaq::channel::ChannelInfo> {
        self.config.devices.get_mut(device)?.hardware.channel_info_mut(channel)
    }

    /// Follow a device's new name on every plot that draws one of its channels.
    fn rename_device_on_plots(&mut self, was: &str, now: &str) {
        if was == now {
            return;
        }
        for plot in self.plots.iter_mut() {
            for plotted in plot.channels.iter_mut() {
                if plotted.reference.device == was {
                    plotted.reference.device = now.to_string();
                }
            }
        }
        self.layout_changed();
    }

    /// The same for one channel of one device.
    fn rename_channel_on_plots(&mut self, device: &str, was: &str, now: &str) {
        if was == now {
            return;
        }
        for plot in self.plots.iter_mut() {
            for plotted in plot.channels.iter_mut() {
                if plotted.reference.device == device && plotted.reference.channel == was {
                    plotted.reference.channel = now.to_string();
                }
            }
        }
        self.layout_changed();
    }

    /// Take every channel of a device off every plot.
    fn forget_device_on_plots(&mut self, device: &str) {
        for plot in self.plots.iter_mut() {
            while let Some(position) = plot
                .channels
                .iter()
                .position(|plotted| plotted.reference.device == device)
            {
                plot.remove_channel(position);
            }
        }
        self.layout_changed();
    }

    /// Take one channel off every plot it is on.
    fn forget_channel_on_plots(&mut self, reference: &ChannelRef) {
        for plot in self.plots.iter_mut() {
            while let Some(position) =
                plot.channels.iter().position(|plotted| &plotted.reference == reference)
            {
                plot.remove_channel(position);
            }
        }
        self.layout_changed();
    }

    /// What generates one mock channel's values, to change.
    fn mock_input_mut(
        &mut self,
        device: usize,
        channel: usize,
    ) -> Option<&mut MockHardwareInput> {
        match self.config.devices.get_mut(device).map(|device| &mut device.hardware) {
            Some(HardwareConfig::MockHardware(mock)) => {
                mock.channels.get_mut(channel).map(|channel| &mut channel.input)
            }
            _ => None,
        }
    }

    /// A channel's number as text, for the field that edits it.
    ///
    /// One draft serves every backend because only one channel is selected at
    /// a time, and none of them has two free-typed numbers.
    fn channel_number(&self, device: usize, channel: usize) -> String {
        match self.config.devices.get(device).map(|device| &device.hardware) {
            Some(HardwareConfig::MockHardware(mock)) => mock
                .channels
                .get(channel)
                .and_then(|channel| MockInputKind::number(&channel.input))
                .map(|number| number.to_string())
                .unwrap_or_default(),
            Some(HardwareConfig::SerialStream(serial)) => serial
                .channels
                .get(channel)
                .map(|channel| channel.index.to_string())
                .unwrap_or_default(),
            _ => String::new(),
        }
    }

    /// Note that the rig's configuration differs from what is on disk.
    ///
    /// Separate from the layout only in what gets written; both settle on the
    /// same delay. Nothing here reaches the devices being read right now — a
    /// run holds its own `Daq`, built from the config as it was when it
    /// started, so a change takes effect when the run is started again.
    fn rig_changed(&mut self) {
        // Rebuilt here rather than kept from launch. The channels a plot
        // offers to add come from the setup, so renaming one has to rename it
        // in the list somebody picks from — and adding or deleting one has to
        // show up there too. Every edit to the rig comes through here, which
        // is what makes this the one place it needs doing.
        self.available = self.config.available_inputs();
        self.rig_dirty_since = Some(Instant::now());
    }

    /// Write the configuration out, if it has settled since the last change.
    fn save_rig_if_settled(&mut self) {
        match self.rig_dirty_since {
            Some(changed) if changed.elapsed() >= SAVE_DELAY => {}
            _ => return,
        }
        self.rig_dirty_since = None;

        let Some(project) = self.project() else {
            return;
        };

        let path = project.config_path();
        match lumberdaq::configuration::write_configuration_file(&path, &self.config) {
            Ok(()) => self.note(format!("saved {}", path.display())),
            Err(error) => self.note(format!("could not save the configuration: {}", error)),
        }
    }

    /// Note that the layout differs from what is on disk.
    ///
    /// The write itself waits for `SAVE_DELAY` to pass without another change,
    /// so a burst of typing becomes one write rather than one per keystroke.
    fn layout_changed(&mut self) {
        self.dirty_since = Some(Instant::now());
    }

    /// Write the layout out, if it has settled since the last change.
    fn save_layout_if_settled(&mut self) {
        match self.dirty_since {
            Some(changed) if changed.elapsed() >= SAVE_DELAY => {}
            _ => return,
        }
        self.dirty_since = None;

        let saved = plot_config::PlotConfig {
            version: plot_config::VERSION,
            history_seconds: self.window_seconds as u64,
            // Read back out of the grid, so what is written is where the panes
            // actually are rather than where they were put.
            layout: layout_from(self.plot_panes.layout(), &self.plot_panes),
            plots: self
                .plots
                .iter()
                .map(|plot| plot_config::Plot {
                    number: plot.number,
                    // Only a name somebody chose. Writing out the "Plot 3" a
                    // reader would have produced anyway just makes the file
                    // noisier than the layout it describes.
                    name: match plot.name == format!("Plot {}", plot.number) {
                        true => None,
                        false => Some(plot.name.clone()),
                    },
                    channels: plot
                        .channels
                        .iter()
                        .map(|plotted| plotted.reference.clone())
                        .collect(),
                })
                .collect(),
        };

        let Some(project) = self.project() else {
            // Nothing open. The arrangement is still on screen and still
            // correct; there is simply nowhere for it to live yet.
            return;
        };

        match project.write_layout(&saved) {
            Ok(path) => self.note(format!("saved {}", path.display())),
            Err(problem) => self.note(problem.to_string()),
        }
    }

    /// A pane's heading row, inset from the edges like the content below it.
    ///
    /// The row's own alignment is left alone: most headings want their parts
    /// centred against each other, but one holding text of two sizes wants
    /// them sharing a baseline instead.
    fn pane_heading<'a>(&self, row: Row<'a, Message>) -> Element<'a, Message> {
        row.padding([8, 10]).into()
    }

    /// A whole pane: heading, a rule the full width of it, then the content.
    ///
    /// The rule is outside the padding on purpose. Inset, it reads as a line
    /// belonging to the title; edge to edge, it separates the pane's heading
    /// from its body the way a heading rule should.
    /// A pane whose body fills it rather than scrolling inside it.
    ///
    /// For content that lays itself out to the space available — a grid of
    /// plots — where a scrollable would be asking something of unbounded
    /// height to fit in a box of unbounded height.
    fn pane_filling<'a>(
        &self,
        title: Row<'a, Message>,
        body: Element<'a, Message>,
    ) -> Element<'a, Message> {
        column![
            self.pane_heading(title),
            pane_rule(),
            container(body).padding(10).width(Fill).height(Fill),
        ]
        .into()
    }

    fn pane<'a>(&self, title: Row<'a, Message>, body: Element<'a, Message>) -> Element<'a, Message> {
        column![
            self.pane_heading(title),
            pane_rule(),
            // Scrolls under a heading that stays put, rather than the heading
            // scrolling away with it.
            scrollable(container(body).padding(10).width(Fill)).height(Fill),
        ]
        .into()
    }

    /// The divider between the log and everything above it, and where it sits.
    fn log_split(&self) -> Option<(pane_grid::Split, f32)> {
        match self.panes.layout() {
            pane_grid::Node::Split { id, ratio, .. } => Some((*id, *ratio)),
            pane_grid::Node::Pane(_) => None,
        }
    }

    /// Add a line to the run log, dropping the oldest once it is full.
    fn note(&mut self, text: String) {
        self.log.push(LogEntry { at: Utc::now(), text });
        // The oldest goes, since the newest is the one being read.
        if self.log.len() > LOG_LIMIT {
            self.log.remove(0);
        }
    }

    /// File one batch against the channel it came from.
    ///
    /// Matched by name because that is the only identity a `Batch` carries,
    /// and the same pair of names lumberdaq's own `ChannelRef` uses.
    fn record(&mut self, device_name: &str, channel_name: &str, datapoints: &[DataPoint]) {
        let Some(channel) = find_channel(&mut self.devices, device_name, channel_name) else {
            return;
        };

        channel.samples += datapoints.len();
        if let Some(point) = datapoints.last() {
            channel.latest = Some(point.value);
        }

        let reference = self.reference;
        let cutoff = seconds_since(reference, Utc::now()) - self.window_seconds;

        // Every plot showing this channel, since a channel may be on several.
        for plot in self.plots.iter_mut() {
            let Some(widget) = plot.widget.as_mut() else { continue };

            for plotted in plot.channels.iter() {
                if plotted.reference.device != device_name
                    || plotted.reference.channel != channel_name
                {
                    continue;
                }

                // Ignored rather than unwrapped: a series that has gone missing
                // is a trace that stops drawing, not a reason to take the
                // interface down in the middle of a run.
                let _ = widget.update_series(&plotted.series, |trace| {
                    for point in datapoints {
                        trace
                            .positions
                            .push([seconds_since(reference, point.datetime), point.value]);
                    }
                    trace.positions.retain(|position| position[0] >= cutoff);
                });
            }
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        // Watching the pointer costs nothing when nothing moves, so it is
        // always on.
        let mut listening = vec![event::listen_with(watch_pointer)];

        // Frames are expensive: iced rebuilds the whole widget tree for every
        // message, so subscribing to them is asking for that sixty times a
        // second. Worth it only when something has to happen without anybody
        // touching the interface - a run to collect from and scroll, or a
        // change waiting out `SAVE_DELAY` before it is written. Idle on a
        // saved project, neither is true and the app stops asking to be
        // redrawn entirely.
        let reading = self.run != RunState::Stopped;
        let owed = self.dirty_since.is_some() || self.rig_dirty_since.is_some();
        if reading || owed {
            listening.push(window::frames().map(|_| Message::Tick));
        }

        Subscription::batch(listening)
    }

    /// The dialog for how the application itself looks.
    ///
    /// Everything in here takes effect on the spot and is written out on the
    /// spot: there is no OK to press, because there is nothing to agree to.
    /// Seeing the change is the confirmation, and closing is just closing.
    fn settings_dialog(&self) -> Element<'_, Message> {
        let step = self.settings.font_step.clamp(1, settings::FONT_STEPS);

        container(
            column![
                row![
                    text("Settings").size(16),
                    space::horizontal(),
                    button(x().size(14))
                        .style(button::text)
                        .padding(2)
                        .on_press(Message::SettingsClosed),
                ]
                .align_y(Center),
                column![
                    text("Theme").size(13),
                    pick_list(Theme::ALL, Some(self.settings.theme()), Message::ThemeChanged)
                        .style(field_pick_style)
                        .text_size(14)
                        .width(Fill),
                ]
                .spacing(6),
                column![
                    row![
                        text("Text size").size(13),
                        space::horizontal(),
                        text(format!("{} of {}", step, settings::FONT_STEPS)).size(13),
                    ],
                    // A slider rather than a list: the steps are ordered and
                    // the only question is bigger or smaller, which a slider
                    // answers without opening anything. It also sidesteps the
                    // question of which way a dropdown would open, which in a
                    // dialog is never a settled one.
                    slider(1..=settings::FONT_STEPS, step, Message::FontStepChanged),
                    text("Scales the whole interface, so spacing and icons keep up with the text.")
                        .size(11),
                ]
                .spacing(6),
            ]
            .spacing(16)
            .width(320),
        )
        .padding(20)
        .style(dialog_style)
        .into()
    }

    /// The dialog for adding a device.
    ///
    /// A type has to be chosen before there is anything to add: it decides
    /// what the rest of the settings are, so there is no sensible device to
    /// create without one. Hence the Add button being dead until it is picked.
    fn add_device_dialog(&self, chosen: Option<&'static str>) -> Element<'_, Message> {
        container(
            column![
                row![
                    text("Add device").size(16),
                    space::horizontal(),
                    button(x().size(14))
                        .style(button::text)
                        .padding(2)
                        .on_press(Message::AddDeviceCancelled),
                ]
                .align_y(Center),
                text("What kind of hardware is it?").size(13),
                pick_list(
                    HardwareConfig::TYPE_NAMES.to_vec(),
                    chosen,
                    Message::AddDeviceTypeChosen
                )
                .placeholder("Choose a type")
                .style(field_pick_style)
                .style(field_pick_style)
                .text_size(14)
                .width(Fill),
                row![
                    space::horizontal(),
                    button(text("Add").size(14))
                        .padding(6)
                        .on_press_maybe(chosen.map(|_| Message::AddDeviceConfirmed)),
                ],
            ]
            .spacing(12)
            .width(320),
        )
        .padding(16)
        .style(dialog_style)
        .into()
    }

    /// What the open right click menu offers, for whatever it was opened on.
    ///
    /// Every entry stays in place and is greyed when it cannot be used, so the
    /// menu is the same shape each time — "Add channel" being unavailable
    /// during a run is worth saying, where a menu that quietly loses an entry
    /// just looks different.
    fn context_entries(&self, target: ContextMenu) -> Element<'_, Message> {
        let editable = self.rig_editable();
        let when = |allowed: bool, message: Message| match allowed {
            true => Some(message),
            false => None,
        };

        context_menu(match target {
            ContextMenu::Device(index) => vec![
                ("Add channel", when(editable, Message::ChannelAdded(index))),
                ("Delete device", when(editable, Message::DeviceDeleted(index))),
            ],
            ContextMenu::Channel(device, channel) => vec![(
                "Delete channel",
                when(editable, Message::ChannelDeleted(device, channel)),
            )],
            // A plot is the interface's own, not the rig's, so it goes whether
            // or not a run is in progress.
            ContextMenu::Plot(index) => {
                vec![("Delete plot", Some(Message::PlotDeleted(index)))]
            }
        })
    }

    /// Whether the rig's settings can be changed at the moment.
    ///
    /// A run holds the devices on its own thread and was built from the config
    /// as it stood when it started, so editing during one would be describing
    /// a rig that is not the one being read. Shown either way: what a device is
    /// set to is worth seeing most while it is running.
    fn rig_editable(&self) -> bool {
        self.run == RunState::Stopped
    }

    /// One field of the rig's configuration.
    ///
    /// A `text_input` with no `on_input` is iced's own way of saying a field
    /// cannot be typed into: it still shows its value and can be read from.
    fn rig_field<'a>(
        &self,
        label: &'a str,
        value: &'a str,
        on_input: impl Fn(String) -> Message + 'a,
    ) -> Element<'a, Message> {
        self.rig_field_explained(label, None, value, on_input)
    }

    /// The same, with an explanation reachable by hovering the label.
    fn rig_field_explained<'a>(
        &self,
        label: &'a str,
        explanation: Option<&'a str>,
        value: &'a str,
        on_input: impl Fn(String) -> Message + 'a,
    ) -> Element<'a, Message> {
        let field = match self.rig_editable() {
            true => text_input(label, value).on_input(on_input).size(14).style(field_style),
            false => text_input(label, value).size(14).style(field_style),
        };

        column![
            match explanation {
                Some(explanation) => labelled(label, explanation),
                None => field_label(label),
            },
            field,
        ]
        .spacing(2)
        .into()
    }

    /// The settings for one device.
    fn device_settings(&self, index: usize) -> Element<'_, Message> {
        let Some(device) = self.config.devices.get(index) else {
            return text("Nothing selected.").size(14).into();
        };

        // A config written elsewhere may hold an interval that is not one of
        // the choices, and it is not this program's business to round it.
        let mut intervals = READ_INTERVALS.to_vec();
        if !intervals.contains(&device.read_interval_ms) {
            intervals.push(device.read_interval_ms);
            intervals.sort_unstable();
        }

        // A pick_list has no disabled state, so a run shows the value as text.
        let interval: Element<'_, Message> = match self.rig_editable() {
            true => pick_list(intervals, Some(device.read_interval_ms), move |interval| {
                Message::DeviceIntervalChanged(index, interval)
            })
            .style(field_pick_style)
            .text_size(14)
            .width(Fill)
            .into(),
            false => text(format!("{} ms", device.read_interval_ms)).size(14).into(),
        };

        column![
            // What it turned out to be if the hardware could say, and what the
            // config says it is otherwise. Not a field: nobody types this, and
            // a device that says it is a USB-6002 is not to be argued with.
            column![
                match &self.model {
                    Some(model) => text(model.clone()).size(16),
                    None => text(device.hardware.describe()).size(16),
                },
                // How it samples, which the model alone does not say. Left out
                // when it is already the heading.
                match self.model.is_some() {
                    true => text(device.hardware.describe()).size(13),
                    false => text(String::new()).size(13),
                },
            ]
            .spacing(2),
            self.rig_field("Name", &device.info.name, move |name| {
                Message::DeviceRenamed(index, name)
            }),
            // Named for what it is: how often lumberdaq collects, which is the
            // sample rate only for hardware that samples on being read.
            column![labelled("Read interval (ms)", "How often lumberdaq collects from this device. For hardware that samples when read, this is also its sample rate."), interval].spacing(2),
            // Below the shared fields: everything from here differs by
            // backend, and there is no form that fits all of them.
            match &device.hardware {
                HardwareConfig::MockHardware(mock) => self.mock_device_settings(index, mock),
                HardwareConfig::SerialStream(serial) => {
                    self.serial_device_settings(index, serial)
                }
                HardwareConfig::NiDaqmx(ni) => self.ni_device_settings(index, ni),
                other => text(format!("{} settings are not editable yet.", other.type_name()))
                    .size(13)
                    .into(),
            },
            // The backend's own verdict on its channels, rather than rules
            // repeated here. Shown as it is found, so a differential pair on
            // an even channel is said now and not at connect.
            match device.hardware.channel_problem() {
                Some(problem) => Element::from(text(problem).size(13).color(PALETTE[1])),
                None => space::horizontal().width(0).into(),
            },
            // Only when stopped, like every other change to the rig, and it
            // takes its channels off the plots with it.
            hint(
                button(trash_two().size(16)).style(button::danger).padding(6).on_press_maybe(
                    match self.rig_editable() {
                        true => Some(Message::DeviceDeleted(index)),
                        false => None,
                    },
                ),
                "Delete this device and its channels",
            ),
            match self.rig_editable() {
                true => Element::from(space::horizontal().width(0)),
                false => field_label("Stop the run to change these"),
            },
        ]
        .spacing(8)
        .into()
    }

    /// The settings a serial device has that others do not.
    fn serial_device_settings<'a>(
        &'a self,
        index: usize,
        serial: &'a lumberdaq::hardware::serial_stream::SerialStreamConfig,
    ) -> Element<'a, Message> {
        // The ports that are plugged in, plus whatever the config names if it
        // is not among them. A rig configured at the bench and opened on a
        // laptop should still show the port it is set to, rather than looking
        // blank as though nobody had chosen.
        let mut options: Vec<String> = self.ports.iter().map(|port| port.label()).collect();
        let current = self
            .ports
            .iter()
            .find(|port| port.name == serial.port)
            .map(|port| port.label())
            .unwrap_or_else(|| serial.port.clone());

        if !serial.port.is_empty() && !options.contains(&current) {
            options.push(current.clone());
        }

        let port: Element<'_, Message> = match self.rig_editable() {
            true => pick_list(options, Some(current), move |label| {
                // The label carries the product name for the human; the config
                // wants only the port, so it is taken back off here.
                let port = label.split(" — ").next().unwrap_or(&label).to_string();
                Message::SerialPortChosen(index, port)
            })
            .placeholder("Choose a port")
            .style(field_pick_style)
            .style(field_pick_style)
            .text_size(14)
            .width(Fill)
            .into(),
            false => text(current).size(14).into(),
        };

        let mut bauds = BAUD_RATES.to_vec();
        if !bauds.contains(&serial.baudrate) {
            bauds.push(serial.baudrate);
            bauds.sort_unstable();
        }

        let baud: Element<'_, Message> = match self.rig_editable() {
            true => pick_list(bauds, Some(serial.baudrate), move |baudrate| {
                Message::SerialBaudChosen(index, baudrate)
            })
            .style(field_pick_style)
            .text_size(14)
            .width(Fill)
            .into(),
            false => text(serial.baudrate.to_string()).size(14).into(),
        };

        column![
            column![
                field_label("Port"),
                // Beside the list rather than in a menu: the usual reason a
                // port is missing is that it was not plugged in yet, and the
                // answer is to plug it in and look again.
                row![
                    port,
                    hint(
                        button(refresh_cw().size(14))
                            .style(button::text)
                            .padding(4)
                            .on_press(Message::PortsRefreshed),
                        "Look for serial ports again",
                    ),
                ]
                .spacing(4)
                .align_y(Center),
            ]
            .spacing(2),
            match self.ports.is_empty() {
                true => text("No serial ports found.").size(13),
                false => text(format!("{} port(s) found", self.ports.len())).size(13),
            },
            column![field_label("Baud rate"), baud].spacing(2),
            // Free text because it is a regular expression: there is no list
            // of the right answers, and the one in the config may be anything.
            self.rig_field("Frame pattern", &serial.frame_pattern, move |pattern| {
                Message::SerialPatternEdited(index, pattern)
            }),
        ]
        .spacing(8)
        .into()
    }

    /// The settings an NI device has that others do not.
    fn ni_device_settings<'a>(
        &'a self,
        index: usize,
        ni: &'a lumberdaq::hardware::ni_daqmx::NiDaqmxConfig,
    ) -> Element<'a, Message> {
        // Offered as a list when the driver can say what is attached, and
        // typed when it cannot — which is every machine without NI software,
        // where the name still has to be enterable.
        let chooser: Element<'_, Message> = match self.ni_devices.is_empty()
            || !self.rig_editable()
        {
            true => self.rig_field("Device", &ni.device, move |name| {
                Message::NiDeviceEdited(index, name)
            }),
            false => {
                let mut names = self.ni_devices.clone();
                if !ni.device.is_empty() && !names.contains(&ni.device) {
                    names.push(ni.device.clone());
                }

                column![
                    field_label("Device"),
                    pick_list(names, Some(ni.device.clone()), move |name| {
                        Message::NiDeviceEdited(index, name)
                    })
                    .style(field_pick_style)
                    .text_size(14)
                    .width(Fill),
                ]
                .spacing(2)
                .into()
            }
        };

        column![
            row![
                chooser,
                hint(
                    button(refresh_cw().size(14))
                        .style(button::text)
                        .padding(4)
                        .on_press(Message::NiDevicesRefreshed),
                    "Look for NI devices again",
                ),
            ]
            .spacing(4)
            .align_y(iced::Bottom),
            // Said plainly, because it decides what the channel list offers and
            // which input a differential pair takes.
            match self.ni_inputs {
                Some(inputs) => text(format!("{} analog inputs", inputs)).size(13),
                None => text("Input count unknown until the device is attached.").size(13),
            },
            match self.ni_devices.is_empty() {
                true => text("No NI devices found. Is the DAQmx runtime installed?").size(13),
                false => text(format!("{} device(s) found", self.ni_devices.len())).size(13),
            },
        ]
        .spacing(8)
        .into()
    }

    /// The settings a mock device has that others do not.
    fn mock_device_settings<'a>(
        &'a self,
        index: usize,
        mock: &'a lumberdaq::hardware::mock_hardware::MockHardwareConfig,
    ) -> Element<'a, Message> {
        let mode = match mock.acquisition {
            mock_hardware::Acquisition::Polled => MockMode::Polled,
            mock_hardware::Acquisition::Streaming { .. } => MockMode::Streaming,
        };

        let mode_field: Element<'_, Message> = match self.rig_editable() {
            true => pick_list(vec![MockMode::Polled, MockMode::Streaming], Some(mode), move |mode| {
                Message::MockModeChanged(index, mode)
            })
            .style(field_pick_style)
            .text_size(14)
            .width(Fill)
            .into(),
            false => text(mode.to_string()).size(14).into(),
        };

        // Only streaming keeps a schedule of its own; a polled device samples
        // when it is read, so there would be nothing for this to say.
        let interval: Element<'_, Message> = match mock.acquisition {
            mock_hardware::Acquisition::Polled => space::horizontal().width(0).into(),
            mock_hardware::Acquisition::Streaming { sample_interval_ms } => {
                let mut choices = MOCK_INTERVALS.to_vec();
                if !choices.contains(&sample_interval_ms) {
                    choices.push(sample_interval_ms);
                    choices.sort_unstable();
                }

                let field: Element<'_, Message> = match self.rig_editable() {
                    true => pick_list(choices, Some(sample_interval_ms), move |interval| {
                        Message::MockIntervalChanged(index, interval)
                    })
                    .style(field_pick_style)
                    .text_size(14)
                    .width(Fill)
                    .into(),
                    false => text(format!("{} ms", sample_interval_ms)).size(14).into(),
                };

                column![field_label("Sample interval (ms)"), field].spacing(2).into()
            }
        };

        column![column![labelled("Acquisition", "Polled takes a sample when asked. Streaming keeps its own schedule and is drained."), mode_field].spacing(2), interval]
            .spacing(8)
            .into()
    }

    /// Which field of a serial frame a channel reads.
    ///
    /// Typed rather than picked from a list: a frame can have as many fields
    /// as the device sends, so any list would be a guess at where to stop.
    fn serial_channel_settings(&self, device: usize, channel: usize) -> Element<'_, Message> {
        column![
            self.rig_field_explained(
                "Frame Index",
                Some("Which field of the frame this channel reads, counting from zero."),
                &self.number_draft,
                move |text| Message::SerialIndexEdited(device, channel, text),
            ),
        ]
        .spacing(8)
        .into()
    }

    /// Which input a Pico channel reads, and how.
    fn pico_channel_settings(
        &self,
        device: usize,
        channel: usize,
        pico: &pico_hrdl::PicoHrdlChannel,
    ) -> Element<'_, Message> {
        // What the attached unit actually has, or everything the backend allows
        // when nothing has said. Offering sixteen on an ADC-20 invites choosing
        // an input that is not there, which is only found out at connect.
        let highest = self.pico_inputs.unwrap_or(pico_hrdl::HIGHEST_CHANNEL);
        let mut numbers: Vec<u16> = (pico_hrdl::LOWEST_CHANNEL..=highest).collect();
        if !numbers.contains(&pico.channel) {
            numbers.push(pico.channel);
            numbers.sort_unstable();
        }
        let editable = self.rig_editable();

        let number: Element<'_, Message> = match editable {
            true => pick_list(numbers, Some(pico.channel), move |number| {
                Message::PicoChannelChosen(device, channel, number)
            })
            .style(field_pick_style)
            .text_size(14)
            .width(Fill)
            .into(),
            false => text(pico.channel.to_string()).size(14).into(),
        };

        let range: Element<'_, Message> = match editable {
            true => pick_list(PICO_RANGES.to_vec(), Some(PicoRange(pico.range)), move |range| {
                Message::PicoRangeChosen(device, channel, range)
            })
            .style(field_pick_style)
            .text_size(14)
            .width(Fill)
            .into(),
            false => text(PicoRange(pico.range).to_string()).size(14).into(),
        };

        column![
            column![field_label("Analog input"), number].spacing(2),
            column![field_label("Range"), range].spacing(2),
            // A differential pair starts on an odd channel and takes the one
            // above it; whether this one may is the backend's to say, and it
            // says so through the problem line on the device.
            checkbox(pico.single_ended)
                .label("Single ended")
                .text_size(14)
                .on_toggle_maybe(match editable {
                    true => Some(move |single| Message::PicoSingleEnded(device, channel, single)),
                    false => None,
                }),
        ]
        .spacing(8)
        .into()
    }

    /// Which input an NI channel reads, and how.
    fn ni_channel_settings(
        &self,
        device: usize,
        channel: usize,
        ni: &lumberdaq::hardware::ni_daqmx::NiDaqmxChannel,
    ) -> Element<'_, Message> {
        let editable = self.rig_editable();
        // The device's own inputs where the driver has said, and a plain
        // sixteen where it has not — a guess either way, but one that stops
        // being a guess as soon as the hardware is attached.
        let mut numbers: Vec<u32> = (0..self.ni_inputs.unwrap_or(16) as u32).collect();
        if !numbers.contains(&ni.channel) {
            numbers.push(ni.channel);
            numbers.sort_unstable();
        }

        let number: Element<'_, Message> = match editable {
            true => pick_list(numbers, Some(ni.channel), move |number| {
                Message::NiChannelChosen(device, channel, number)
            })
            .style(field_pick_style)
            .text_size(14)
            .width(Fill)
            .into(),
            false => text(format!("ai{}", ni.channel)).size(14).into(),
        };

        let mut ranges: Vec<NiRange> = NI_RANGES.iter().copied().map(NiRange).collect();
        if !ranges.iter().any(|range| range.0 == ni.range) {
            ranges.push(NiRange(ni.range));
        }

        let range: Element<'_, Message> = match editable {
            true => pick_list(ranges, Some(NiRange(ni.range)), move |range| {
                Message::NiRangeChosen(device, channel, range)
            })
            .style(field_pick_style)
            .text_size(14)
            .width(Fill)
            .into(),
            false => text(NiRange(ni.range).to_string()).size(14).into(),
        };

        column![
            column![field_label("Analog input (ai)"), number].spacing(2),
            column![text("Range").size(13), range].spacing(2),
            // NI pairs a channel with the one four above it, so on an eight
            // input device only ai0 to ai3 can start a pair.
            checkbox(ni.single_ended)
                .label("Single ended")
                .text_size(14)
                .on_toggle_maybe(match editable {
                    true => Some(move |single| Message::NiSingleEnded(device, channel, single)),
                    false => None,
                }),
        ]
        .spacing(8)
        .into()
    }

    /// What a mock channel generates, and the number that shapes it.
    fn mock_channel_settings<'a>(
        &'a self,
        device: usize,
        channel: usize,
        input: &MockHardwareInput,
    ) -> Element<'a, Message> {
        let kind = MockInputKind::of(input);

        let kind_field: Element<'_, Message> = match self.rig_editable() {
            true => pick_list(
                vec![MockInputKind::Random, MockInputKind::Constant, MockInputKind::Sine],
                Some(kind),
                move |kind| Message::MockInputChanged(device, channel, kind),
            )
            .style(field_pick_style)
            .text_size(14)
            .width(Fill)
            .into(),
            false => text(kind.to_string()).size(14).into(),
        };

        let number: Element<'_, Message> = match kind.number_label() {
            None => space::horizontal().width(0).into(),
            // The draft rather than the config's value, so the field can hold
            // "1." while somebody is partway through typing "1.5".
            Some(label) => self.rig_field(label, &self.number_draft, move |text| {
                Message::MockNumberEdited(device, channel, text)
            }),
        };

        column![column![labelled("Input", "What generates this channel's values. Sine and Constant take the number below."), kind_field].spacing(2), number].spacing(8).into()
    }

    /// The settings for one channel.
    fn channel_settings(&self, device: usize, channel: usize) -> Element<'_, Message> {
        // Borrowed from the config rather than copied out of it: a text_input
        // holds its value for as long as the widget lives, which is longer
        // than a local would.
        let Some(info) = self
            .config
            .devices
            .get(device)
            .and_then(|device| device.hardware.channel_info(channel))
        else {
            return text("Nothing selected.").size(14).into();
        };

        // Judged by the library rather than here: building a channel from this
        // description runs exactly the checks a run would, so a formula that
        // passes will also start. Said as it is typed, rather than at connect.
        let problem = match info.scale.is_some() {
            false => None,
            true => Channel::from_info(info.clone()).err().map(|error| error.to_string()),
        };

        column![
            text("Channel").size(16),
            self.rig_field("Name", &info.name, move |name| {
                Message::ChannelRenamed(device, channel, name)
            }),
            // Directly above the unit it produces, because that is the pair:
            // the formula decides what the recorded number is and the unit
            // says what it means. A scale replaces the measurement rather than
            // adding a channel, so this belongs to the channel's own settings
            // rather than to a thing of its own.
            self.rig_field_explained(
                "Formula",
                Some(
                    "The measurement is written x, so x * 5 + 5 records five times the reading \
                     plus five, in the unit below. The raw reading is not kept. Leave empty to \
                     record the measurement as it is.",
                ),
                info.scale.as_ref().map(|scale| scale.equation()).unwrap_or(""),
                move |equation| Message::ScaleEdited(device, channel, equation),
            ),
            match problem {
                Some(problem) => Element::from(text(problem).size(13).color(PALETTE[1])),
                None => space::horizontal().width(0).into(),
            },
            self.rig_field("Unit", &info.unit, move |unit| {
                Message::ChannelUnitChanged(device, channel, unit)
            }),
            // A scale carrying named constants from a sensor definition shows
            // where they came from and what they are, read only: editing those
            // wants a form of its own rather than a list.
            match info.scale.as_ref().and_then(|scale| scale.from()) {
                Some(from) => field_label(from),
                None => Element::from(space::horizontal().width(0)),
            },
            match info.scale.as_ref().and_then(|scale| scale.parameters()) {
                None => Element::from(space::horizontal().width(0)),
                Some(parameters) => column(parameters.iter().map(|(name, value)| {
                    row![
                        text(name.as_str()).size(13),
                        space::horizontal(),
                        text(value.to_string()).size(13)
                    ]
                    .into()
                }))
                .spacing(2)
                .into(),
            },
            // Everything below differs by backend: a mock channel's input, a
            // serial channel's field index, a Pico's channel number and how it
            // pairs for a differential reading. One form cannot ask all of it.
            match self.config.devices.get(device).map(|device| &device.hardware) {
                Some(HardwareConfig::MockHardware(mock)) => match mock.channels.get(channel) {
                    Some(mock_channel) => {
                        self.mock_channel_settings(device, channel, &mock_channel.input)
                    }
                    None => space::horizontal().width(0).into(),
                },
                Some(HardwareConfig::SerialStream(serial)) => match serial.channels.get(channel) {
                    Some(_) => self.serial_channel_settings(device, channel),
                    None => space::horizontal().width(0).into(),
                },
                Some(HardwareConfig::PicoHrdl(pico)) => match pico.channels.get(channel) {
                    Some(pico_channel) => {
                        self.pico_channel_settings(device, channel, pico_channel)
                    }
                    None => space::horizontal().width(0).into(),
                },
                Some(HardwareConfig::NiDaqmx(ni)) => match ni.channels.get(channel) {
                    Some(ni_channel) => self.ni_channel_settings(device, channel, ni_channel),
                    None => space::horizontal().width(0).into(),
                },
                Some(HardwareConfig::None) => space::horizontal().width(0).into(),
                None => space::horizontal().width(0).into(),
            },
            hint(
                button(trash_two().size(16)).style(button::danger).padding(6).on_press_maybe(
                    match self.rig_editable() {
                        true => Some(Message::ChannelDeleted(device, channel)),
                        false => None,
                    }
                ),
                "Delete this channel",
            ),
            match self.rig_editable() {
                true => Element::from(space::horizontal().width(0)),
                false => field_label("Stop the run to change these"),
            },
        ]
        .spacing(8)
        .into()
    }

    /// The settings that apply to every plot at once.
    fn all_plots_settings(&self) -> Element<'_, Message> {
        let current = self.window_seconds as u64;

        // A layout may have been written with a span that is not one of the
        // choices. Offering only the choices would mean the setting appeared
        // blank, or worse, quietly changed on the first look at it.
        let mut spans = HISTORY_CHOICES.to_vec();
        if !spans.contains(&current) {
            spans.push(current);
            spans.sort_unstable();
        }

        column![
            text("All plots").size(16),
            labelled("History", "How far back every plot goes. One setting for all of them, as the saved layout keeps one."),
            pick_list(spans, Some(current), Message::HistoryChanged)
                .style(field_pick_style)
                .text_size(14)
                .width(Fill),
        ]
        .spacing(8)
        .into()
    }

    /// The settings for one plot, shown in the configuration panel.
    fn plot_settings<'a>(&'a self, index: usize, plot: &'a AppPlot) -> Element<'a, Message> {
        let channels = match plot.channels.is_empty() {
            true => column![text("No channels yet.").size(14)],
            false => column(plot.channels.iter().enumerate().map(|(position, plotted)| {
                row![
                    text(plotted.reference.to_string()).size(14),
                    space::horizontal(),
                    button(circle_minus().size(14))
                        .style(button::text)
                        .padding(2)
                        .on_press(Message::ChannelRemovedFromPlot(index, position)),
                ]
                .align_y(Center)
                .into()
            }))
            .spacing(4),
        };

        // Only what is not already on this plot, so the list is what can
        // actually be added rather than everything with some of it inert.
        let addable: Vec<ChannelRef> = self
            .available
            .iter()
            .filter(|reference| {
                !plot.channels.iter().any(|plotted| plotted.reference == **reference)
            })
            .cloned()
            .collect();

        let add: Element<'_, Message> = match addable.is_empty() {
            true => text("Every channel is on this plot.").size(13).into(),
            false => pick_list(addable, None::<ChannelRef>, move |reference| {
                Message::ChannelAddedToPlot(index, reference)
            })
            .placeholder("Add a channel")
            .style(field_pick_style)
            .style(field_pick_style)
            .text_size(14)
            .width(Fill)
            .into(),
        };

        column![
            field_label("Name"),
            text_input("Plot name", &plot.name)
                .style(field_style)
                .on_input(move |name| Message::PlotRenamed(index, name)),
            field_label("Channels"),
            // The list and the thing that adds to it share one tinted panel,
            // so they read as one control rather than as a list with a
            // stray dropdown under it. The contents are inset so the tint
            // shows around them and the grouping is obvious.
            container(column![channels, add].spacing(8).padding(6))
                .width(Fill)
                .style(|theme: &Theme| container::Style {
                    background: Some(field_colour(theme).into()),
                    border: Border {
                        radius: FIELD_RADIUS.into(),
                        width: 1.0,
                        color: theme.extended_palette().background.weak.color,
                    },
                    ..container::Style::default()
                }),
            button(text("Delete plot").size(14))
                .style(button::danger)
                .on_press(Message::PlotDeleted(index)),
        ]
        .spacing(8)
        .into()
    }

    /// One plot as a pane: its name and legend on the title bar, its traces
    /// below, sharing a background so each reads as one block.
    /// The `'a` is spelled out because the returned widgets borrow from *both*
    /// arguments — the channel names from `plot`, the traces from `self` —
    /// while elision would tie the result to `&self` alone.
    fn plot_card<'a>(
        &'a self,
        index: usize,
        plot: &'a AppPlot,
    ) -> pane_grid::Content<'a, Message> {
        let name = text(&plot.name);

        // Our own legend, since iced_plot's sits over the y axis and offers no
        // way to move it. A short rule in the trace colour, then the channel
        // it belongs to.
        let legend = row(plot.channels.iter().map(|plotted| {
            let colour = plotted.colour;

            row![
                container(space::horizontal().width(14).height(3)).style(
                    move |_theme: &Theme| container::Style {
                        background: Some(colour.into()),
                        ..container::Style::default()
                    }
                ),
                text(plotted.reference.channel.as_str()).size(13),
            ]
            .spacing(6)
            .align_y(Center)
            .into()
        }))
        .spacing(16)
        .align_y(Center);

        let traces: Element<'_, Message> = match &plot.widget {
            Some(widget) => widget.view().map(move |message| Message::Plot(index, message)),
            None => container(text("No channels on this plot").size(14))
                .center_x(Fill)
                .center_y(Fill)
                .into(),
        };

        let is_selected = self.selected == Some(Selection::Plot(index));

        // The name row is the pane's title bar rather than part of its body,
        // because a pane can only be dragged by its title bar — so this makes
        // the plot's own name the handle you pick it up by.
        // Right click opens the plot's menu. Only the right button, so the
        // left one is still free to start a drag on the same bar.
        let title_bar = pane_grid::TitleBar::new(
            MouseArea::new(row![name, space::horizontal(), legend].align_y(Center))
                .on_right_press(Message::ContextOpened(ContextMenu::Plot(index))),
        )
        .padding(10)
        .style(|theme: &Theme| container::Style {
            background: Some(card_colour(theme).into()),
            ..container::Style::default()
        });

        pane_grid::Content::new(container(traces).padding(padding::all(10).top(0)))
            .title_bar(title_bar)
            .style(move |theme: &Theme| {
                let palette = theme.extended_palette();

                container::Style {
                    background: Some(card_colour(theme).into()),
                    border: Border {
                        radius: 8.0.into(),
                        // The selected plot is the one the settings panel is
                        // talking about, so it has to be obvious which that is.
                        width: if is_selected { 2.0 } else { 0.0 },
                        color: palette.primary.base.color,
                    },
                    ..container::Style::default()
                }
            })
    }

    fn view(&self) -> Element<'_, Message> {
        // The app name is the way into the file menu, with the chevron saying
        // so. Styled as text rather than as a button, because a menu bar is
        // read as a name until it is used.
        let title = button(
            row![
                text("Lumberjack").size(20),
                // Always down: it says "there is a menu here", which stays
                // true while the menu is open.
                chevron_down().size(14),
            ]
            .spacing(6)
            .align_y(Center),
        )
        .style(button::text)
        .padding(4)
        .on_press(Message::FileMenuToggled);

        // Disabled while stopping rather than hidden, so the control does not
        // move about and it is clear why pressing it does nothing.
        // Three buttons that are always the same three buttons. What changes is
        // their colour, which says what the rig is doing rather than what the
        // press would do — so the header is a state to read at a glance, not a
        // control that rearranges itself under the cursor.
        let run_control = row![
            transport(
                play().size(16).into(),
                self.run == RunState::Running,
                button::success,
                "Start acquisition",
                Message::RunStarted,
            ),
            transport(
                square().size(16).into(),
                false,
                button::secondary,
                "Stop acquisition and recording",
                Message::RunStopped,
            ),
            transport(
                circle().size(16).into(),
                self.recording(),
                button::danger,
                "Record to disk",
                Message::RecordPressed,
            ),
        ]
        .spacing(6);

        // Recording is a thing you do to a run, so it is only offered while
        // one is going. `on_press_maybe(None)` leaves the button there but
        // dead, rather than having the header rearrange itself.
        // Application example
        let header = container(
            row![
                title,
                space::horizontal(),
                run_control,
            ]
            .spacing(15)
            .padding(5)
            .align_y(Center),
        );

        // Each pane's content is built fresh here rather than once above,
        // since this closure runs once per pane and a Button/Element isn't
        // Copy — there's nothing to share between the three arms anyway.
        let pane_grid = PaneGrid::new(&self.panes, |_id, kind, _is_maximized| {
            let content: Element<'_, Message> = match kind {
                PaneKind::Devices => {
                    let add_device_button = hint(
                        button(circle_plus())
                            .style(button::text)
                            .padding(4)
                            .on_press(Message::AddDeviceOpened),
                        "Add a device",
                    );

                    let device_list =
                        column(self.devices.iter().enumerate().map(|(index, device)| {
                            // Expanding and inspecting are different intentions,
                            // so they get different targets: the chevron opens
                            // the device, the name selects it.
                            let chevron: Element<'_, Message> = match device.channels.is_empty() {
                                true => space::horizontal().width(22).into(),
                                false => button(match device.expanded {
                                    true => chevron_down().size(14),
                                    false => chevron_right().size(14),
                                })
                                .style(button::text)
                                .padding(4)
                                .on_press(Message::ToggleDevice(index))
                                .into(),
                            };

                            let selected = self.selected == Some(Selection::Device(index));
                            let device_header = row![
                                chevron,
                                button(text(&device.name))
                                    .style(match selected {
                                        true => button::primary,
                                        false => button::text,
                                    })
                                    .padding(4)
                                    .width(Fill)
                                    .on_press(Message::DeviceSelected(index)),
                                // Adding a channel is a change to the rig like
                                // any other, so it waits for the run to stop.
                                hint(
                                    button(circle_plus().size(14))
                                        .style(button::text)
                                        .padding(4)
                                        .on_press_maybe(match self.rig_editable() {
                                            true => Some(Message::ChannelAdded(index)),
                                            false => None,
                                        }),
                                    "Add a channel to this device",
                                ),
                            ]
                            .align_y(Center);

                            let mut entry = column![MouseArea::new(device_header)
                                .on_right_press(Message::ContextOpened(ContextMenu::Device(
                                    index
                                )))];

                            if device.expanded {
                                entry = entry.push(
                                    column(device.channels.iter().enumerate().map(
                                        |(position, channel)| {
                                            let reading = match channel.latest {
                                                Some(value) => {
                                                    format!("{:.3} {}", value, channel.unit)
                                                }
                                                None => "—".to_string(),
                                            };

                                            let selected = self.selected
                                                == Some(Selection::Channel(index, position));

                                            let row = button(
                                                row![
                                                    // The icon does the
                                                    // indenting, so a channel
                                                    // reads as belonging to
                                                    // the device above it
                                                    // without a margin as well.
                                                    square_arrow_right().size(11),
                                                    text(&channel.name).size(14),
                                                    space::horizontal(),
                                                    // Dressed as a field, since
                                                    // that is what it is: a
                                                    // value belonging to this
                                                    // channel rather than more
                                                    // of its name.
                                                    container(text(reading).size(13))
                                                        .padding([2, 6])
                                                        .style(|theme: &Theme| container::Style {
                                                            background: Some(
                                                                field_colour(theme).into(),
                                                            ),
                                                            border: Border {
                                                                radius: FIELD_RADIUS.into(),
                                                                width: 1.0,
                                                                color: theme
                                                                    .extended_palette()
                                                                    .background
                                                                    .weak
                                                                    .color,
                                                            },
                                                            ..container::Style::default()
                                                        }),
                                                ]
                                                // Keeps the name off the
                                                // reading when the pane is
                                                // dragged narrow: the flexible
                                                // space between them goes to
                                                // nothing, this does not.
                                                .spacing(8)
                                                .align_y(Center),
                                            )
                                            .style(match selected {
                                                true => button::primary,
                                                false => button::text,
                                            })
                                            .padding(4)
                                            .width(Fill)
                                            .on_press(Message::ChannelSelected(index, position));

                                            let row = MouseArea::new(row).on_right_press(
                                                Message::ContextOpened(ContextMenu::Channel(
                                                    index, position,
                                                )),
                                            );

                                            row.into()
                                        },
                                    ))
                                    .spacing(4)
                                    // Indented as well as iconed: the icon says
                                    // what these are, the indent says what they
                                    // are under. Enough to sit inboard of the
                                    // device name without pushing the reading
                                    // off the end of a narrow pane.
                                    .padding(padding::left(14)),
                                );
                            }

                            entry.into()
                        }))
                        .spacing(5);

                    self.pane(
                        row![text("Devices"), space::horizontal(), add_device_button]
                            .align_y(Center),
                        device_list.into(),
                    )
                }
                PaneKind::Config => {
                    // Not named `settings`: that is the gear icon's function.
                    let panel: Element<'_, Message> = match self.selected {
                        Some(Selection::Plot(index)) => match self.plots.get(index) {
                            Some(plot) => self.plot_settings(index, plot),
                            // The selected plot was deleted from under us.
                            None => text("Nothing selected.").size(14).into(),
                        },
                        Some(Selection::AllPlots) => self.all_plots_settings(),
                        Some(Selection::Device(index)) => self.device_settings(index),
                        Some(Selection::Channel(device, channel)) => {
                            self.channel_settings(device, channel)
                        }
                        None => {
                            text("Select a device, channel or plot to configure it.").size(14).into()
                        }
                    };

                    self.pane(row![text("Configuration")].align_y(Center), panel)
                }
                PaneKind::Log => {
                    let samples: usize = self
                        .devices
                        .iter()
                        .flat_map(|device| device.channels.iter())
                        .map(|channel| channel.samples)
                        .sum();

                    let chevron = button(match self.log_open {
                        true => chevron_down().size(14),
                        false => chevron_right().size(14),
                    })
                    .style(button::text)
                    .padding(2)
                    .on_press(Message::LogToggled);

                    // Shut, the pane is its heading and nothing else — no rule
                    // either, since there is nothing under it to divide from.
                    // The newest line rides along on the bar, so a run can be
                    // watched without opening it.
                    if !self.log_open {
                        let latest = match self.log.last() {
                            Some(entry) => entry.text.clone(),
                            None => String::new(),
                        };

                        return pane_grid::Content::new(self.pane_heading(
                            row![chevron, text("Log"), text(latest).size(13), space::horizontal()]
                                // So the newest line reads as a separate thing
                                // from the title rather than running on from it.
                                .spacing(12)
                                // Bottom rather than centre: the line is
                                // smaller than the title, and centring it
                                // leaves it floating above the title's
                                // baseline instead of sitting on it.
                                .align_y(Bottom),
                        ))
                        .style(container::rounded_box);
                    }

                    let lines = column(self.log.iter().map(|entry| {
                        row![
                            text(entry.at.format("%H:%M:%S").to_string()).size(13),
                            text(&entry.text).size(13),
                        ]
                        .spacing(10)
                        .into()
                    }))
                    .spacing(2);

                    return pane_grid::Content::new(
                        column![
                            self.pane_heading(
                                row![
                                    chevron,
                                    text("Log"),
                                    space::horizontal(),
                                    text(format!("{} samples", samples)).size(14),
                                ]
                                .align_y(Center),
                            ),
                            pane_rule(),
                            // Anchored to the bottom so the newest line is the
                            // one in view, the way a terminal behaves. Cheaper
                            // and steadier than scrolling there on every line.
                            scrollable(lines.width(Fill).padding(10))
                                .anchor_bottom()
                                .height(Fill),
                        ]
                        .width(Fill),
                    )
                    .style(container::rounded_box);
                }
                PaneKind::Data => {
                    let add_plot_button = hint(
                        button(circle_plus())
                            .style(button::text)
                            .padding(4)
                            .on_press(Message::AddPlot),
                        "Add a plot",
                    );

                    let plot_settings_button = hint(
                        button(settings())
                            .style(button::text)
                            .padding(4)
                            .on_press(Message::AllPlotsSelected),
                        "Settings for all plots",
                    );

                    // A grid of its own inside this pane. The plots arrange
                    // against each other; the sidebar is not somewhere a plot
                    // can be dragged.
                    let cards = PaneGrid::new(&self.plot_panes, |_pane, number, _maximised| {
                        let found = self
                            .plots
                            .iter()
                            .enumerate()
                            .find(|(_, plot)| plot.number == *number);

                        match found {
                            Some((index, plot)) => self.plot_card(index, plot),
                            // A pane whose plot has gone. Closed on the next
                            // deletion rather than drawn as an error.
                            None => pane_grid::Content::new(space::horizontal().width(0)),
                        }
                    })
                    .width(Fill)
                    .height(Fill)
                    .spacing(8)
                    .on_click(Message::PlotClicked)
                    .on_drag(Message::PlotDragged)
                    .on_resize(10, Message::PlotsResized);

                    self.pane_filling(
                        row![
                            text("Plots"),
                            space::horizontal(),
                            plot_settings_button,
                            add_plot_button
                        ]
                        .align_y(Center),
                        cards.into(),
                    )
                }
            };

            pane_grid::Content::new(content).style(container::rounded_box)
        })
        .width(Fill)
        .height(Fill)
        .spacing(4)
        .on_resize(10, Message::PaneResized);

        let screen = column![header, container(pane_grid).padding(4).height(Fill)];

        // Floated over the interface at the pointer rather than pushed into the
        // list, so opening one does not move the thing it was opened on.
        let screen: Element<'_, Message> = match self.context {
            None => screen.into(),
            // The menu sits inside a layer covering the whole window, so a
            // click anywhere but the menu lands on that layer and closes it.
            // The same shape as the dialog, and for the same reason: what
            // catches the click has to be above everything that would
            // otherwise take it first.
            Some(target) => stack![
                screen,
                opaque(
                    MouseArea::new(
                        container(opaque(self.context_entries(target)))
                            .width(Fill)
                            .height(Fill)
                            .padding(padding::left(self.context_at.x).top(self.context_at.y))
                    )
                    .on_press(Message::ContextDismissed)
                ),
            ]
            .into(),
        };

        // Anchored under the name it drops from rather than at the pointer:
        // this one belongs to a place on screen, not to whatever was clicked.
        let screen: Element<'_, Message> = match self.file_menu {
            false => screen,
            true => stack![
                screen,
                opaque(
                    MouseArea::new(
                        container(opaque(menu(
                            vec![
                                MenuItem::Entry("New project", Some(Message::ProjectCreated)),
                                MenuItem::Entry("Load project", Some(Message::ProjectOpened)),
                                MenuItem::Entry(
                                    "Export",
                                    self.project.as_ref().map(|_| Message::Exported),
                                ),
                                MenuItem::Divider,
                                MenuItem::Entry("Settings", Some(Message::SettingsOpened)),
                            ],
                            170.0,
                        )))
                        .width(Fill)
                        .height(Fill)
                        .padding(padding::left(8).top(44))
                    )
                    .on_press(Message::ContextDismissed)
                ),
            ]
            .into(),
        };

        // With nothing open there is nothing to look at, so the interface asks
        // for a project rather than showing empty panels and leaving somebody
        // to find the menu. Not dismissable, because there is no state behind
        // it to work in.
        let screen: Element<'_, Message> = match self.project.is_some() {
            true => screen,
            false => stack![
                screen,
                opaque(
                    container(opaque(self.welcome()))
                        .width(Fill)
                        .height(Fill)
                        .center_x(Fill)
                        .align_y(Top)
                        .padding(60)
                ),
            ]
            .into(),
        };

        let screen: Element<'_, Message> = match self.settings_open {
            false => screen,
            // Dismissed by clicking away as well as by the x, like the others.
            // Nothing in here is half-finished, so leaving is never a loss.
            true => stack![
                screen,
                opaque(
                    MouseArea::new(
                        container(opaque(self.settings_dialog()))
                            .width(Fill)
                            .height(Fill)
                            .center_x(Fill)
                            .align_y(Top)
                            .padding(60)
                    )
                    .on_press(Message::SettingsClosed)
                ),
            ]
            .into(),
        };

        match self.adding_device {
            None => screen,
            // Laid over the window rather than shown beside it, so nothing
            // behind can be clicked while a device is half-made. Clicking away
            // cancels, the same as the x.
            //
            // Held near the top rather than centred, which is what decides
            // which way its dropdown opens: iced compares the room below a
            // pick_list against the room above, and the room below has the
            // control's own height taken off it. A centred control therefore
            // always has less room below and always opens upwards.
            Some(chosen) => stack![
                screen,
                opaque(
                    MouseArea::new(
                        container(opaque(self.add_device_dialog(chosen)))
                            .width(Fill)
                            .height(Fill)
                            .center_x(Fill)
                            .align_y(Top)
                            .padding(60)
                    )
                    .on_press(Message::AddDeviceCancelled)
                )
            ]
            .into(),
        }
        }

        fn theme(&self) -> Option<Theme> {
        Some(self.settings.theme())
    }

    /// How much bigger or smaller than usual to draw everything.
    fn scale_factor(&self) -> f32 {
        self.settings.scale_factor()
    }

    /// Keep what was just chosen, and say so if it could not be kept.
    ///
    /// Written straight away rather than on exit: settings changed and then
    /// lost to a crash are worse than a handful of small writes.
    fn remember_settings(&mut self) {
        if let Err(problem) = self.settings.save() {
            self.note(format!("could not save the settings: {}", problem));
        }
    }
}

impl Drop for AppDaq {
    /// Ask the run to end. The process is going away anyway, but a recording
    /// run would want its sink flushed rather than cut off mid-batch.
    fn drop(&mut self) {
        if let Some(acquisition) = self.acquisition.as_ref() {
            acquisition.stop.store(true, Ordering::Relaxed);
        }
    }
}
