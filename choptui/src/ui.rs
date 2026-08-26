//! The display: what it holds, how it draws, and the thread that runs it.
//!
//! Drawing happens here and nowhere else. The thread collecting data sends
//! updates and never touches the terminal, so the two run at their own rates:
//! a rig sampling at 1 kHz does not ask for a thousand redraws, and a redraw
//! that takes a moment does not delay a write to disk.

use crate::monitor::Update;
use lumberdaq::config::DaqConfig;
use ratatui::crossterm::event::{ self, Event, KeyCode, KeyEventKind, KeyModifiers };
use ratatui::layout::{ Alignment, Constraint, Layout, Rect };
use ratatui::style::{ Color, Modifier, Style };
use ratatui::text::{ Line, Span };
use ratatui::widgets::{ Block, Paragraph, Row, Table };
use ratatui::Frame;
use std::collections::VecDeque;
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
                        })
                        .collect(),
                })
                .collect(),
            log: VecDeque::new(),
            tab: 0,
        }
    }

    pub fn apply(&mut self, update: Update) {
        match update {
            Update::Data { device, channel, value, added } => {
                if let Some(row) = self.channel_mut(&device, &channel) {
                    row.latest = Some(value);
                    row.readings += added;
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

fn devices(frame: &mut Frame, area: Rect, state: &State) {
    // Two lines of border plus one per channel, so each device keeps its own
    // height rather than the space being shared out evenly.
    let heights: Vec<Constraint> = state
        .devices
        .iter()
        .map(|device| Constraint::Length(device.channels.len() as u16 + 2))
        .chain(std::iter::once(Constraint::Min(0)))
        .collect();

    let areas = Layout::vertical(heights).split(area);
    for (device, area) in state.devices.iter().zip(areas.iter()) {
        let (label, colour) = match &device.status {
            Status::Connected => ("Connected".to_string(), Color::Green),
            Status::Disconnected(None) => ("Waiting".to_string(), DIM),
            Status::Disconnected(Some(cause)) => (cause.clone(), Color::Red),
        };

        let block = Block::bordered()
            .border_style(Style::new().fg(ACCENT))
            .title(Span::styled(
                format!(" {} ", device.name),
                Style::new().fg(Color::White).add_modifier(Modifier::BOLD),
            ))
            .title_top(
                Line::from(Span::styled(format!(" {} ", label), Style::new().fg(colour)))
                    .right_aligned(),
            );

        let rows = device.channels.iter().map(|channel| {
            Row::new(vec![
                Span::styled(channel.name.clone(), Style::new().fg(ACCENT)),
                Span::styled(
                    match channel.latest {
                        Some(value) => format!("{:.3} {}", value, channel.unit),
                        // Nothing yet, which is not a reading of zero.
                        None => format!("--- {}", channel.unit),
                    },
                    Style::new().fg(Color::White),
                ),
                Span::styled(channel.readings.to_string(), Style::new().fg(DIM)),
            ])
        });

        frame.render_widget(
            Table::new(
                rows,
                [Constraint::Percentage(50), Constraint::Length(18), Constraint::Length(10)],
            )
            .block(block),
            *area,
        );
    }
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

    fn state() -> State {
        State {
            project: "test_projects/scaled".to_string(),
            devices: vec![
                DeviceRow {
                    name: "Rig".to_string(),
                    status: Status::Connected,
                    channels: vec![
                        ChannelRow {
                            name: "Flow".to_string(),
                            unit: "L/min".to_string(),
                            latest: Some(14.5),
                            readings: 63,
                        },
                        ChannelRow {
                            name: "Pressure".to_string(),
                            unit: "bar".to_string(),
                            latest: Some(7.25),
                            readings: 61,
                        },
                    ],
                },
                DeviceRow {
                    name: "Missing rig".to_string(),
                    status: Status::Disconnected(Some("port not found".to_string())),
                    channels: vec![ChannelRow {
                        name: "Temperature".to_string(),
                        unit: "C".to_string(),
                        latest: None,
                        readings: 0,
                    }],
                },
            ],
            log: VecDeque::new(),
            tab: 0,
        }
    }

    #[test]
    fn the_devices_tab_shows_every_channel_and_its_latest_reading() {
        let screen = rendered(&state(), 74, 12);
        println!("{}", screen);
        assert!(screen.contains("LUMBERJACK"), "{}", screen);
        assert!(screen.contains("14.500 L/min"), "{}", screen);
        assert!(screen.contains("Connected"), "{}", screen);
    }

    #[test]
    fn a_device_that_never_connected_still_appears_with_its_channels() {
        // Otherwise the one thing worth looking at, a rig that is not there,
        // would be the one thing the screen does not mention.
        let screen = rendered(&state(), 74, 12);
        assert!(screen.contains("Missing rig"), "{}", screen);
        assert!(screen.contains("Temperature"), "{}", screen);
        assert!(screen.contains("port not found"), "{}", screen);
    }

    #[test]
    fn nothing_yet_reads_differently_from_a_reading_of_zero() {
        let mut state = state();
        state.devices[0].channels[0].latest = Some(0.0);
        let screen = rendered(&state, 74, 12);
        assert!(screen.contains("0.000 L/min"), "{}", screen);
        assert!(screen.contains("--- C"), "{}", screen);
    }

    #[test]
    fn readings_update_the_row_they_belong_to() {
        let mut state = state();
        state.apply(Update::Data {
            device: "Rig".to_string(),
            channel: "Flow".to_string(),
            value: 21.0,
            added: 5,
        });
        assert_eq!(state.devices[0].channels[0].latest, Some(21.0));
        assert_eq!(state.devices[0].channels[0].readings, 68);
    }

    #[test]
    fn a_reading_for_something_not_in_the_setup_is_ignored() {
        // A calculated device is not in `devices`, so its batches arrive naming
        // a channel the screen has no row for. Dropping them beats panicking.
        let mut state = state();
        state.apply(Update::Data {
            device: "Derived".to_string(),
            channel: "Delta P".to_string(),
            value: 1.0,
            added: 1,
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
        let screen = rendered(&state, 74, 12);
        assert!(screen.contains("the port went away"), "{}", screen);
        state.tab = 2;
        let screen = rendered(&state, 74, 12);
        assert!(screen.contains("Rig disconnected: the port went away"), "{}", screen);
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
