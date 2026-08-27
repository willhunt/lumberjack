//! The display: what it holds, how it draws, and the thread that runs it.
//!
//! Drawing happens here and nowhere else. The thread collecting data sends
//! updates and never touches the terminal, so the two run at their own rates:
//! a rig sampling at 1 kHz does not ask for a thousand redraws, and a redraw
//! that takes a moment does not delay a write to disk.

use crate::monitor::Update;
use chrono::{ DateTime, Utc };
use lumberdaq::config::DaqConfig;
use lumberdaq::datapoint::DataPoint;
use ratatui::crossterm::event::{ self, Event, KeyCode, KeyEventKind, KeyModifiers };
use ratatui::layout::{ Alignment, Constraint, Layout, Rect };
use ratatui::style::{ Color, Modifier, Style };
use ratatui::text::{ Line, Span };
use ratatui::symbols::Marker;
use ratatui::widgets::{ Axis, Block, Chart, Dataset, GraphType, Padding, Paragraph, Row, Table };
use ratatui::Frame;
use std::collections::{ BTreeMap, VecDeque };
use std::sync::atomic::{ AtomicBool, Ordering };
use std::sync::mpsc::{ Receiver, RecvTimeoutError };
use std::time::{ Duration, Instant };

/// The longest the screen goes without being redrawn, and how often the
/// keyboard is looked at. Short enough that a key press feels immediate.
const TICK: Duration = Duration::from_millis(50);

/// How many past events are kept. Enough to scroll back through a bad patch,
/// not so many that a device reconnecting all night fills memory.
const LOG_KEPT: usize = 200;

const TABS: [&str; 4] = ["Devices", "Plots", "Log", "Settings"];

/// Space between a channel name, its reading and its count.
const GAP: u16 = 2;

/// Room for a count of readings. Six figures is an hour at 50 Hz.
const COUNT_WIDTH: u16 = 7;

/// The most of a disconnection reason to put on a device border. The whole of
/// it is in the log; a driver sentence on the border would otherwise set the
/// width of every box on the screen.
const REASON_SHOWN: usize = 28;

/// How many readings a channel keeps for drawing.
///
/// A plot is at most a couple of hundred columns wide and braille gives two
/// dots per column, so beyond this there is nothing more to see. It also means
/// the window a plot covers is set by the sample rate: 600 readings is twelve
/// seconds at 50 Hz and half a second at 1 kHz. The axis says which.
const HISTORY_KEPT: usize = 600;

/// Room for the pointer showing which channel the keys act on. Always there,
/// so a row does not shift sideways when it becomes the selected one.
const POINTER_WIDTH: u16 = 2;

/// Room for a plot number in brackets.
const PLOT_WIDTH: u16 = 3;

/// Room for the legend beside a plot: a channel name and its latest reading.
const LEGEND_WIDTH: u16 = 30;

/// Series colours, in the order channels are added to a plot.
const SERIES: [Color; 6] = [
    Color::LightBlue,
    Color::LightGreen,
    Color::LightYellow,
    Color::LightMagenta,
    Color::LightCyan,
    Color::White,
];

const ACCENT: Color = Color::LightRed;
const DIM: Color = Color::DarkGray;

pub enum Status {
    Connected,
    /// Tried and failed, or lost. The cause is kept so the screen can say why
    /// rather than only that something is wrong.
    Disconnected(Option<String>),
}

pub struct ChannelRow {
    pub name: String,
    pub unit: String,
    /// None until the first reading arrives, which is not the same as zero.
    pub latest: Option<f64>,
    pub readings: usize,
    /// Which plot this is drawn on, if any. Set from the Devices tab.
    pub plot: Option<usize>,
    /// Recent readings, oldest first, for drawing.
    ///
    /// Kept for every channel rather than only the plotted ones. It costs a
    /// few kilobytes each, and it means assigning a channel to a plot shows
    /// what it has been doing rather than an empty box that fills up slowly.
    pub history: VecDeque<DataPoint>,
}

pub struct DeviceRow {
    pub name: String,
    pub status: Status,
    pub channels: Vec<ChannelRow>,
}

pub struct State {
    pub project: String,
    pub devices: Vec<DeviceRow>,
    pub log: VecDeque<String>,
    pub tab: usize,
    /// Which channel the Devices tab is pointing at, counted across every
    /// device rather than within one.
    pub selected: usize,
    /// The first reading seen, which the time axis counts from. Taken from the
    /// data rather than from when the display started, so the axis says when a
    /// reading was taken and not when it was drawn.
    origin: Option<DateTime<Utc>>,
}

impl State {
    /// Everything the setup says exists, before any of it has been heard from.
    ///
    /// Built from the config rather than from arriving data, so a device that
    /// never connects still appears with its channels rather than the screen
    /// simply not mentioning it.
    pub fn from_config(project: &str, config: &DaqConfig) -> State {
        State {
            project: project.to_string(),
            devices: config
                .devices
                .iter()
                .map(|device| DeviceRow {
                    name: device.info.name.clone(),
                    status: Status::Disconnected(None),
                    channels: device
                        .hardware
                        .channel_infos()
                        .into_iter()
                        .map(|info| ChannelRow {
                            name: info.name,
                            unit: info.unit,
                            latest: None,
                            readings: 0,
                            plot: None,
                            history: VecDeque::new(),
                        })
                        .collect(),
                })
                .collect(),
            log: VecDeque::new(),
            tab: 0,
            selected: 0,
            origin: None,
        }
    }

    pub fn apply(&mut self, update: Update) {
        match update {
            Update::Data { device, channel, points } => {
                if self.origin.is_none() {
                    self.origin = points.first().map(|point| point.datetime);
                }
                if let Some(row) = self.channel_mut(&device, &channel) {
                    row.latest = points.last().map(|point| point.value);
                    row.readings += points.len();
                    for point in points {
                        if row.history.len() == HISTORY_KEPT {
                            row.history.pop_front();
                        }
                        row.history.push_back(point);
                    }
                }
            }
            Update::Connected { device } => {
                self.note(format!("{} connected", device));
                self.set_status(&device, Status::Connected);
            }
            Update::Disconnected { device, cause } => {
                self.note(match &cause {
                    Some(cause) => format!("{} disconnected: {}", device, cause),
                    None => format!("{} disconnected", device),
                });
                self.set_status(&device, Status::Disconnected(cause));
            }
            // A problem is not a disconnection: the port is fine and the device
            // keeps being read, so the status is left alone.
            Update::Problem { device, message } => self.note(format!("{}: {}", device, message)),
        }
    }

    /// Every channel in the setup, in the order they appear on screen.
    pub fn channels(&self) -> impl Iterator<Item = &ChannelRow> {
        self.devices.iter().flat_map(|device| device.channels.iter())
    }

    /// Move the pointer on the Devices tab, staying inside the list.
    pub fn move_selection(&mut self, by: isize) {
        let count = self.channels().count();
        if count == 0 {
            return;
        }
        let last = count as isize - 1;
        self.selected = (self.selected as isize + by).clamp(0, last) as usize;
    }

    /// Put the channel being pointed at on a plot, or take it off one.
    pub fn assign(&mut self, plot: Option<usize>) {
        let selected = self.selected;
        if let Some(row) = self
            .devices
            .iter_mut()
            .flat_map(|device| device.channels.iter_mut())
            .nth(selected)
        {
            row.plot = plot;
        }
    }

    /// Seconds from the first reading of the run, which is what a time axis
    /// counts in.
    fn seconds(&self, point: &DataPoint) -> f64 {
        match self.origin {
            Some(origin) => (point.datetime - origin).num_milliseconds() as f64 / 1000.0,
            None => 0.0,
        }
    }

    fn channel_mut(&mut self, device: &str, channel: &str) -> Option<&mut ChannelRow> {
        self.devices
            .iter_mut()
            .find(|row| row.name == device)?
            .channels
            .iter_mut()
            .find(|row| row.name == channel)
    }

    fn set_status(&mut self, device: &str, status: Status) {
        if let Some(row) = self.devices.iter_mut().find(|row| row.name == device) {
            row.status = status;
        }
    }

    fn note(&mut self, message: String) {
        if self.log.len() == LOG_KEPT {
            self.log.pop_front();
        }
        self.log.push_back(message);
    }
}

/// Draw one frame.
///
/// Kept apart from the loop so it can be rendered into a buffer and checked
/// without a terminal to run in.
pub fn draw(frame: &mut Frame, state: &State) {
    let [header, tabs, body] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(2), Constraint::Min(0)])
            .areas(frame.area());

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("LUMBERJACK", Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)),
            Span::styled(format!("  {}", state.project), Style::new().fg(DIM)),
        ])),
        header,
    );
    // The record button and its timer belong here. Not built yet, and saying so
    // is better than an ornament that does nothing.
    frame.render_widget(
        Paragraph::new(Span::styled("not recording", Style::new().fg(DIM)))
            .alignment(Alignment::Right),
        header,
    );

    frame.render_widget(Paragraph::new(tab_bar(state.tab)), tabs);

    match state.tab {
        0 => devices(frame, body, state),
        1 => plots(frame, body, state),
        2 => log(frame, body, state),
        _ => frame.render_widget(
            Paragraph::new(Span::styled(
                format!("{} is not built yet.", TABS[state.tab]),
                Style::new().fg(DIM),
            ))
            .block(Block::bordered().border_style(Style::new().fg(ACCENT))),
            body,
        ),
    }
}

fn tab_bar(selected: usize) -> Line<'static> {
    let mut spans = Vec::new();
    for (index, name) in TABS.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled("  -  ", Style::new().fg(DIM)));
        }
        spans.push(match index == selected {
            true => Span::styled(*name, Style::new().fg(Color::White).add_modifier(Modifier::BOLD)),
            false => Span::styled(*name, Style::new().fg(ACCENT)),
        });
    }
    Line::from(spans)
}

/// Where each channel sits among those sharing its plot, which is what decides
/// its colour.
///
/// Filtering keeps the order channels are in, so a plot numbers its traces the
/// same way this does, and the box on the Devices tab can be coloured to match
/// the line it produces.
fn series_positions(state: &State) -> Vec<Option<usize>> {
    let mut counted: BTreeMap<usize, usize> = BTreeMap::new();
    state
        .channels()
        .map(|channel| {
            channel.plot.map(|plot| {
                let next = counted.entry(plot).or_insert(0);
                let position = *next;
                *next += 1;
                position
            })
        })
        .collect()
}

fn devices(frame: &mut Frame, area: Rect, state: &State) {
    // Columns are sized to what is in them, and to the widest across every
    // device rather than per device, so the boxes line up with one another
    // instead of each finding its own layout.
    let channels = || state.devices.iter().flat_map(|device| device.channels.iter());
    let name_width = channels()
        .map(|channel| channel.name.chars().count() as u16)
        .max()
        .unwrap_or(0)
        .clamp(8, 32);
    // A reading and its unit: room for a number, a space, and the unit.
    let value_width = channels()
        .map(|channel| channel.unit.chars().count() as u16)
        .max()
        .unwrap_or(0)
        .clamp(1, 12)
        + 11;

    // Content, plus a border and a space of padding at each side.
    let content = POINTER_WIDTH
        + name_width
        + GAP
        + value_width
        + GAP
        + PLOT_WIDTH
        + GAP
        + COUNT_WIDTH;
    let width = state
        .devices
        .iter()
        .map(title_width)
        .max()
        .unwrap_or(0)
        .max(content + 4);

    // Left aligned at its own width rather than stretched across the terminal,
    // which on a wide screen left a gulf between a name and its reading.
    let width = width.min(area.width);

    // Placed one after another rather than by the constraint solver, which
    // when the terminal is too short shares the shortfall out among the boxes
    // until the padding has eaten every channel. A device is better shown
    // whole or not at all.
    let positions = series_positions(state);
    let mut top = area.y;
    let mut shown = 0;
    let mut first = 0;
    for device in state.devices.iter() {
        let height = device.channels.len() as u16 + 4;
        if top + height > area.bottom() {
            break;
        }
        // Where this device sits in the run of channels the pointer moves
        // through, so it can tell whether the selected one is its own.
        let pointing_at = match state.selected.checked_sub(first) {
            Some(within) if within < device.channels.len() => Some(within),
            _ => None,
        };
        render_device(
            frame,
            Rect { x: area.x, y: top, width: width, height: height },
            device,
            name_width,
            value_width,
            pointing_at,
            &positions[first..first + device.channels.len()],
        );
        first += device.channels.len();
        // A blank line between one device and the next.
        top += height + 1;
        shown += 1;
    }

    // Silently leaving devices off screen would look like a setup with fewer
    // devices in it than it has.
    // `top` has already skipped the blank line after the last box, so that
    // line is the first free one and is where this goes. If even that is past
    // the bottom there is nowhere to say it.
    if shown < state.devices.len() && top <= area.bottom() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                format!("{} more below, not enough room", state.devices.len() - shown),
                Style::new().fg(DIM),
            )),
            Rect { x: area.x, y: top - 1, width: width, height: 1 },
        );
    }
}

fn render_device(
    frame: &mut Frame,
    area: Rect,
    device: &DeviceRow,
    name_width: u16,
    value_width: u16,
    pointing_at: Option<usize>,
    positions: &[Option<usize>],
) {
    let (label, colour) = status(&device.status);

    let block = Block::bordered()
        .border_style(Style::new().fg(ACCENT))
        // A blank line above and below the channels, and a space at each side,
        // so nothing sits against the border.
        .padding(Padding::symmetric(1, 1))
        .title(Span::styled(
            format!(" {} ", device.name),
            Style::new().fg(Color::White).add_modifier(Modifier::BOLD),
        ))
        .title_top(
            Line::from(Span::styled(format!(" {} ", label), Style::new().fg(colour)))
                .right_aligned(),
        );

    let rows = device.channels.iter().enumerate().map(|(index, channel)| {
        let selected = pointing_at == Some(index);
        let name = match selected {
            true => Style::new().fg(Color::White).add_modifier(Modifier::BOLD),
            false => Style::new().fg(ACCENT),
        };
        Row::new(vec![
            // A pointer rather than a highlight, so it shows on a terminal
            // that will not colour a background.
            Span::styled(if selected { ">" } else { " " }, Style::new().fg(Color::White)),
            Span::styled(channel.name.clone(), name),
            Span::styled(
                match channel.latest {
                    Some(value) => format!("{:.3} {}", value, channel.unit),
                    // Nothing yet, which is not a reading of zero.
                    None => format!("--- {}", channel.unit),
                },
                Style::new().fg(Color::White),
            ),
            // Coloured as the trace it produces, so the two can be read
            // against each other.
            match (channel.plot, positions[index]) {
                (Some(plot), Some(position)) => Span::styled(
                    format!("[{}]", plot),
                    Style::new().fg(SERIES[position % SERIES.len()]),
                ),
                _ => Span::styled("[-]", Style::new().fg(DIM)),
            },
            Span::styled(channel.readings.to_string(), Style::new().fg(DIM)),
        ])
    });

    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Length(POINTER_WIDTH),
                Constraint::Length(name_width),
                Constraint::Length(value_width),
                Constraint::Length(PLOT_WIDTH),
                Constraint::Length(COUNT_WIDTH),
            ],
        )
        .column_spacing(GAP)
        .block(block),
        area,
    );
}

/// What a device says on its border, and how that should look.
fn status(status: &Status) -> (String, Color) {
    match status {
        Status::Connected => ("Connected".to_string(), Color::Green),
        Status::Disconnected(None) => ("Waiting".to_string(), DIM),
        Status::Disconnected(Some(reason)) => (shortened(reason), Color::Red),
    }
}

fn shortened(reason: &str) -> String {
    match reason.chars().count() > REASON_SHOWN {
        true => reason.chars().take(REASON_SHOWN - 1).collect::<String>() + "\u{2026}",
        false => reason.to_string(),
    }
}

/// How wide a device box has to be for its name and status to fit the border.
fn title_width(device: &DeviceRow) -> u16 {
    let (label, _) = status(&device.status);
    // A space each side of both, two corners, and two dashes between them.
    (device.name.chars().count() + label.chars().count() + 8) as u16
}

fn plots(frame: &mut Frame, area: Rect, state: &State) {
    let mut numbers: Vec<usize> = state.channels().filter_map(|channel| channel.plot).collect();
    numbers.sort_unstable();
    numbers.dedup();

    if numbers.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "Nothing is plotted. On the Devices tab, point at a channel and press 1 to 9.",
                Style::new().fg(DIM),
            ))
            .block(Block::bordered().border_style(Style::new().fg(ACCENT))),
            area,
        );
        return;
    }

    // An even share each. A plot of one channel is no less worth seeing than a
    // plot of four.
    let heights = vec![Constraint::Ratio(1, numbers.len() as u32); numbers.len()];
    for (number, area) in numbers.iter().zip(Layout::vertical(heights).split(area).iter()) {
        render_plot(frame, *area, state, *number);
    }
}

fn render_plot(frame: &mut Frame, area: Rect, state: &State, number: usize) {
    let [drawing, legend] =
        Layout::horizontal([Constraint::Min(20), Constraint::Length(LEGEND_WIDTH)]).areas(area);

    let members: Vec<&ChannelRow> =
        state.channels().filter(|channel| channel.plot == Some(number)).collect();
    // Built before the datasets because a dataset borrows its points, and they
    // have to outlive the widget that reads them.
    let series: Vec<Vec<(f64, f64)>> = members
        .iter()
        .map(|channel| {
            channel.history.iter().map(|point| (state.seconds(point), point.value)).collect()
        })
        .collect();

    let block = Block::bordered().border_style(Style::new().fg(ACCENT)).title(Span::styled(
        format!(" Plot {} ", number),
        Style::new().fg(Color::White).add_modifier(Modifier::BOLD),
    ));

    match span(&series) {
        None => frame.render_widget(
            Paragraph::new(Span::styled(" waiting for readings", Style::new().fg(DIM)))
                .block(block),
            drawing,
        ),
        Some((across, up)) => {
            let datasets: Vec<Dataset> = series
                .iter()
                .enumerate()
                .map(|(position, points)| {
                    Dataset::default()
                        .marker(Marker::Braille)
                        .graph_type(GraphType::Line)
                        .style(Style::new().fg(SERIES[position % SERIES.len()]))
                        .data(points)
                })
                .collect();
            frame.render_widget(
                Chart::new(datasets)
                    .block(block)
                    .x_axis(axis(across, 1, "s"))
                    .y_axis(axis(up, 2, "")),
                drawing,
            );
        }
    }

    // The legend doubles as the reading, which is what the Devices tab would
    // otherwise have to be switched back to for.
    let lines: Vec<Line> = members
        .iter()
        .enumerate()
        .map(|(position, channel)| {
            Line::from(vec![
                Span::styled(
                    channel.name.clone(),
                    Style::new().fg(SERIES[position % SERIES.len()]),
                ),
                Span::styled(
                    match channel.latest {
                        Some(value) => format!("  {:.3} {}", value, channel.unit),
                        None => format!("  --- {}", channel.unit),
                    },
                    Style::new().fg(Color::White),
                ),
            ])
        })
        .collect();
    frame.render_widget(
        Paragraph::new(lines).block(Block::new().padding(Padding::new(1, 0, 1, 0))),
        legend,
    );
}

/// What a plot has to cover, or None when nothing has arrived to draw.
fn span(series: &[Vec<(f64, f64)>]) -> Option<([f64; 2], [f64; 2])> {
    let mut across = [f64::INFINITY, f64::NEG_INFINITY];
    let mut up = [f64::INFINITY, f64::NEG_INFINITY];
    for (x, y) in series.iter().flatten() {
        across = [across[0].min(*x), across[1].max(*x)];
        up = [up[0].min(*y), up[1].max(*y)];
    }
    if !across[0].is_finite() {
        return None;
    }
    // A reading that has not moved, or only one of them, would otherwise be
    // asked to fill an axis of no height at all.
    up = match up[1] - up[0] < f64::EPSILON {
        true => [up[0] - 1.0, up[1] + 1.0],
        false => {
            let room = (up[1] - up[0]) * 0.05;
            [up[0] - room, up[1] + room]
        }
    };
    if across[1] - across[0] < f64::EPSILON {
        across[1] = across[0] + 1.0;
    }
    Some((across, up))
}

fn axis(bounds: [f64; 2], decimals: usize, suffix: &str) -> Axis<'static> {
    Axis::default().style(Style::new().fg(DIM)).bounds(bounds).labels([
        format!("{:.*}{}", decimals, bounds[0], suffix),
        format!("{:.*}{}", decimals, bounds[1], suffix),
    ])
}

fn log(frame: &mut Frame, area: Rect, state: &State) {
    // Newest last, and only as many as fit, so the latest is always on screen
    // without any scrolling to build yet.
    let lines: Vec<Line> = state
        .log
        .iter()
        .rev()
        .take(area.height.saturating_sub(2) as usize)
        .rev()
        .map(|entry| Line::from(Span::styled(entry.clone(), Style::new().fg(Color::White))))
        .collect();

    frame.render_widget(
        Paragraph::new(lines).block(
            Block::bordered()
                .border_style(Style::new().fg(ACCENT))
                .title(Span::styled(" Log ", Style::new().fg(Color::White))),
        ),
        area,
    );
}

/// Run the display until told to stop, or until the operator says so.
///
/// Setting `stop` is how the display asks for the run to end: the same flag
/// Ctrl-C sets, so the devices wind down and the sink gets its final flush
/// rather than the process being killed with data still buffered.
pub fn run(updates: Receiver<Update>, mut state: State, stop: &AtomicBool) -> std::io::Result<()> {
    let mut terminal = ratatui::init();
    let outcome = watch(&mut terminal, updates, &mut state, stop);
    ratatui::restore();
    outcome
}

fn watch(
    terminal: &mut ratatui::DefaultTerminal,
    updates: Receiver<Update>,
    state: &mut State,
    stop: &AtomicBool,
) -> std::io::Result<()> {
    let mut last_drawn = Instant::now() - TICK;
    while !stop.load(Ordering::Relaxed) {
        // Wait for something, then take everything else already waiting. At a
        // high sample rate that is many batches for one redraw, which is the
        // point: the screen cannot show more than it can show.
        match updates.recv_timeout(TICK) {
            Ok(update) => {
                state.apply(update);
                while let Ok(update) = updates.try_recv() {
                    state.apply(update);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            // The run has finished and dropped its senders.
            Err(RecvTimeoutError::Disconnected) => break,
        }

        if last_drawn.elapsed() >= TICK {
            terminal.draw(|frame| draw(frame, state))?;
            last_drawn = Instant::now();
        }

        if event::poll(Duration::ZERO)? {
            if let Event::Key(key) = event::read()? {
                // Windows reports a key going down and coming back up. Without
                // this every press counts twice, and Tab skips a tab.
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                // Raw mode means Ctrl-C arrives as a key rather than as a
                // signal, so unless it is handled here it does nothing at all.
                // It should end a run the way it does everywhere else.
                let interrupt = key.modifiers.contains(KeyModifiers::CONTROL)
                    && key.code == KeyCode::Char('c');
                match key.code {
                    _ if interrupt => stop.store(true, Ordering::Relaxed),
                    KeyCode::Char('q') | KeyCode::Esc => stop.store(true, Ordering::Relaxed),
                    KeyCode::Tab | KeyCode::Right => state.tab = (state.tab + 1) % TABS.len(),
                    KeyCode::BackTab | KeyCode::Left => {
                        state.tab = (state.tab + TABS.len() - 1) % TABS.len()
                    }
                    // Only where the pointer they move is on screen. Changing
                    // something invisible from another tab is worse than the
                    // key doing nothing at all.
                    KeyCode::Up if state.tab == 0 => state.move_selection(-1),
                    KeyCode::Down if state.tab == 0 => state.move_selection(1),
                    KeyCode::Char(key) if state.tab == 0 && key.is_ascii_digit() => {
                        // Zero takes a channel off a plot, there being no plot
                        // zero to put it on.
                        state.assign(match key.to_digit(10).unwrap_or(0) as usize {
                            0 => None,
                            plot => Some(plot),
                        });
                    }
                    KeyCode::Char('-') if state.tab == 0 => state.assign(None),
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    /// Render one frame into a buffer and give back what it looks like.
    ///
    /// The point of keeping `draw` apart from the loop: layout can be checked
    /// on a machine with no terminal, in a test that runs in milliseconds.
    fn rendered(state: &State, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| draw(frame, state)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let mut screen = String::new();
        for row in 0..buffer.area.height {
            for column in 0..buffer.area.width {
                screen.push_str(buffer[(column, row)].symbol());
            }
            screen.push('\n');
        }
        screen
    }

    fn channel(name: &str, unit: &str, latest: Option<f64>, readings: usize) -> ChannelRow {
        ChannelRow {
            name: name.to_string(),
            unit: unit.to_string(),
            latest: latest,
            readings: readings,
            plot: None,
            history: VecDeque::new(),
        }
    }

    /// Readings a second apart, starting at the epoch, so a plot has something
    /// with a known shape to draw.
    fn readings(values: &[f64]) -> Vec<DataPoint> {
        let origin = DateTime::from_timestamp(0, 0).unwrap();
        values
            .iter()
            .enumerate()
            .map(|(second, value)| DataPoint {
                datetime: origin + chrono::Duration::seconds(second as i64),
                value: *value,
            })
            .collect()
    }

    fn state() -> State {
        State {
            project: "test_projects/scaled".to_string(),
            devices: vec![
                DeviceRow {
                    name: "Rig".to_string(),
                    status: Status::Connected,
                    channels: vec![
                        channel("Flow", "L/min", Some(14.5), 63),
                        channel("Pressure", "bar", Some(7.25), 61),
                    ],
                },
                DeviceRow {
                    name: "Missing rig".to_string(),
                    status: Status::Disconnected(Some("port not found".to_string())),
                    channels: vec![channel("Temperature", "C", None, 0)],
                },
            ],
            log: VecDeque::new(),
            tab: 0,
            selected: 0,
            origin: None,
        }
    }

    #[test]
    fn the_devices_tab_shows_every_channel_and_its_latest_reading() {
        let screen = rendered(&state(), 74, 16);
        println!("{}", screen);
        assert!(screen.contains("LUMBERJACK"), "{}", screen);
        assert!(screen.contains("14.500 L/min"), "{}", screen);
        assert!(screen.contains("Connected"), "{}", screen);
    }

    #[test]
    fn a_device_that_never_connected_still_appears_with_its_channels() {
        // Otherwise the one thing worth looking at, a rig that is not there,
        // would be the one thing the screen does not mention.
        let screen = rendered(&state(), 74, 16);
        assert!(screen.contains("Missing rig"), "{}", screen);
        assert!(screen.contains("Temperature"), "{}", screen);
        assert!(screen.contains("port not found"), "{}", screen);
    }

    #[test]
    fn nothing_yet_reads_differently_from_a_reading_of_zero() {
        let mut state = state();
        state.devices[0].channels[0].latest = Some(0.0);
        let screen = rendered(&state, 74, 16);
        assert!(screen.contains("0.000 L/min"), "{}", screen);
        assert!(screen.contains("--- C"), "{}", screen);
    }

    #[test]
    fn readings_update_the_row_they_belong_to() {
        let mut state = state();
        state.apply(Update::Data {
            device: "Rig".to_string(),
            channel: "Flow".to_string(),
            points: readings(&[1.0, 2.0, 21.0]),
        });
        assert_eq!(state.devices[0].channels[0].latest, Some(21.0));
        assert_eq!(state.devices[0].channels[0].readings, 66);
        // Every reading is kept, not just the newest, or a plot would be
        // drawing the drain rate rather than the signal.
        assert_eq!(state.devices[0].channels[0].history.len(), 3);
    }

    #[test]
    fn a_reading_for_something_not_in_the_setup_is_ignored() {
        // A calculated device is not in `devices`, so its batches arrive naming
        // a channel the screen has no row for. Dropping them beats panicking.
        let mut state = state();
        state.apply(Update::Data {
            device: "Derived".to_string(),
            channel: "Delta P".to_string(),
            points: readings(&[1.0]),
        });
        assert_eq!(state.devices[0].channels[0].readings, 63);
    }

    #[test]
    fn losing_a_device_says_why_on_the_status_and_in_the_log() {
        let mut state = state();
        state.apply(Update::Disconnected {
            device: "Rig".to_string(),
            cause: Some("the port went away".to_string()),
        });
        let screen = rendered(&state, 74, 16);
        assert!(screen.contains("the port went away"), "{}", screen);
        state.tab = 2;
        let screen = rendered(&state, 74, 16);
        assert!(screen.contains("Rig disconnected: the port went away"), "{}", screen);
    }

    #[test]
    fn a_box_is_only_as_wide_as_it_needs_to_be() {
        // Stretched across the terminal, a name and its reading ended up at
        // opposite ends of the screen.
        let screen = rendered(&state(), 120, 16);
        let widest = screen
            .lines()
            .filter(|line| line.contains("L/min"))
            .map(|line| line.trim_end().chars().count())
            .max()
            .unwrap();
        assert!(widest < 60, "box is {} wide on a 120 column terminal", widest);
    }

    #[test]
    fn a_long_reason_does_not_set_the_width_of_every_box() {
        // What a serial port really says when it is not there. On the border it
        // would widen every device on screen; the log has it in full.
        let mut state = state();
        state.apply(Update::Disconnected {
            device: "Missing rig".to_string(),
            cause: Some("The system cannot find the file specified.".to_string()),
        });
        let screen = rendered(&state, 120, 16);
        assert!(!screen.contains("cannot find the file specified."), "{}", screen);
        assert!(screen.contains("The system cannot"), "{}", screen);
        assert!(
            state.log.iter().any(|line| line.ends_with("file specified.")),
            "the whole reason should still be in the log"
        );
    }

    #[test]
    fn devices_that_do_not_fit_are_counted_rather_than_dropped() {
        // A screen quietly showing fewer devices than the setup has looks like
        // a setup with fewer devices in it.
        let screen = rendered(&state(), 74, 10);
        assert!(screen.contains("Rig"), "{}", screen);
        assert!(screen.contains("1 more below"), "{}", screen);
    }

    /// Point at a channel and put it on a plot, the way the keys do.
    fn plot(state: &mut State, selected: usize, number: usize) {
        state.selected = selected;
        state.assign(Some(number));
    }

    #[test]
    fn the_pointer_runs_through_every_device_not_just_one() {
        let mut state = state();
        state.move_selection(2);
        // Two channels on the first device, so this is the second device.
        state.assign(Some(1));
        assert_eq!(state.devices[1].channels[0].plot, Some(1));
    }

    #[test]
    fn the_pointer_stops_at_the_ends_rather_than_wrapping() {
        let mut state = state();
        state.move_selection(-1);
        assert_eq!(state.selected, 0);
        state.move_selection(99);
        assert_eq!(state.selected, 2, "three channels, so the last is 2");
    }

    #[test]
    fn nothing_is_plotted_until_something_is_put_on_a_plot() {
        let mut state = state();
        state.tab = 1;
        let screen = rendered(&state, 90, 20);
        assert!(screen.contains("Nothing is plotted"), "{}", screen);
    }

    #[test]
    fn a_plot_draws_its_channels_and_names_them_beside_it() {
        let mut state = state();
        state.apply(Update::Data {
            device: "Rig".to_string(),
            channel: "Flow".to_string(),
            points: readings(&[1.0, 4.0, 2.0, 8.0, 5.0]),
        });
        plot(&mut state, 0, 1);
        state.tab = 1;
        let screen = rendered(&state, 90, 20);
        println!("{}", screen);
        assert!(screen.contains("Plot 1"), "{}", screen);
        // The legend is the reading as well, so the Devices tab does not have
        // to be switched back to for it.
        assert!(screen.contains("Flow"), "{}", screen);
        assert!(screen.contains("5.000 L/min"), "{}", screen);
        // Four seconds of readings, and a y axis spanning what arrived.
        assert!(screen.contains("4.0s"), "{}", screen);
    }

    #[test]
    fn channels_on_different_plots_get_a_plot_each() {
        let mut state = state();
        plot(&mut state, 0, 1);
        plot(&mut state, 1, 3);
        state.tab = 1;
        let screen = rendered(&state, 90, 20);
        assert!(screen.contains("Plot 1"), "{}", screen);
        assert!(screen.contains("Plot 3"), "{}", screen);
    }

    #[test]
    fn a_channel_comes_off_a_plot_again() {
        let mut state = state();
        plot(&mut state, 0, 1);
        assert_eq!(state.devices[0].channels[0].plot, Some(1));
        state.assign(None);
        assert_eq!(state.devices[0].channels[0].plot, None);
    }

    #[test]
    fn a_plot_of_a_reading_that_never_moves_still_has_an_axis() {
        // Otherwise the axis is zero high and there is nowhere to draw.
        let mut state = state();
        state.apply(Update::Data {
            device: "Rig".to_string(),
            channel: "Flow".to_string(),
            points: readings(&[7.0, 7.0, 7.0]),
        });
        plot(&mut state, 0, 1);
        state.tab = 1;
        let screen = rendered(&state, 90, 20);
        assert!(screen.contains("6.00"), "{}", screen);
        assert!(screen.contains("8.00"), "{}", screen);
    }

    #[test]
    fn a_plotted_channel_with_no_readings_yet_says_so() {
        let mut state = state();
        plot(&mut state, 2, 1);
        state.tab = 1;
        let screen = rendered(&state, 90, 20);
        assert!(screen.contains("waiting for readings"), "{}", screen);
    }

    #[test]
    fn history_is_bounded() {
        let mut state = state();
        let values: Vec<f64> = (0..HISTORY_KEPT + 50).map(|n| n as f64).collect();
        state.apply(Update::Data {
            device: "Rig".to_string(),
            channel: "Flow".to_string(),
            points: readings(&values),
        });
        let history = &state.devices[0].channels[0].history;
        assert_eq!(history.len(), HISTORY_KEPT);
        // The oldest go, not the newest.
        assert_eq!(history.back().unwrap().value, (HISTORY_KEPT + 49) as f64);
    }

    #[test]
    fn a_plot_numbers_its_channels_the_same_way_the_devices_tab_colours_them() {
        // The box beside a channel is coloured as the line it produces, which
        // only holds if both count position the same way.
        let mut state = state();
        plot(&mut state, 0, 1);
        plot(&mut state, 2, 1);
        let positions = series_positions(&state);
        assert_eq!(positions[0], Some(0));
        assert_eq!(positions[1], None);
        assert_eq!(positions[2], Some(1));
    }

    #[test]
    fn a_problem_is_logged_without_marking_the_device_down() {
        // The port is fine and the device keeps being read. Showing it as
        // disconnected would be a lie that hides a real disconnection later.
        let mut state = state();
        state.apply(Update::Problem {
            device: "Rig".to_string(),
            message: "frame would not parse".to_string(),
        });
        assert!(matches!(state.devices[0].status, Status::Connected));
        assert_eq!(state.log.len(), 1);
    }
}
