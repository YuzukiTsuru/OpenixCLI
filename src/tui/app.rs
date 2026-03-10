//! App state, main loop, and event handling

use std::io;
use std::time::Instant;

use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::prelude::*;
use tokio::sync::mpsc;

use crate::process::global_progress::{global_progress, set_tui_mode};
use crate::utils::terminal::{self, TuiLogLevel, TuiLogMessage};

use super::bridge;
use super::event::{AppEvent, DeviceInfo, LogLevel};
use super::ui;
use super::widgets::firmware_info::{FirmwareField, FirmwareState};
use super::widgets::log_view::LogState;
use super::widgets::progress::ProgressState;

/// Which panel is focused (only left-side panels support Tab switching)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusPanel {
    Devices,
    Options,
}

impl FocusPanel {
    pub fn toggle(&self) -> Self {
        match self {
            FocusPanel::Devices => FocusPanel::Options,
            FocusPanel::Options => FocusPanel::Devices,
        }
    }
}

/// Application state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppState {
    Idle,
    Ready,
    Flashing,
    Done,
    Error,
}

/// Main application struct
pub struct App {
    pub state: AppState,
    pub devices: Vec<DeviceInfo>,
    pub selected_device: usize,
    pub device_scroll_offset: usize,
    pub firmware: FirmwareState,
    pub progress: ProgressState,
    pub log: LogState,
    pub focus: FocusPanel,
    pub show_help: bool,
    pub input_mode: bool,
    pub input_buffer: String,
    flash_start_time: Option<Instant>,
    packer: Option<crate::firmware::OpenixPacker>,
    should_quit: bool,
}

impl App {
    pub fn new() -> Self {
        Self {
            state: AppState::Idle,
            devices: Vec::new(),
            selected_device: 0,
            device_scroll_offset: 0,
            firmware: FirmwareState::default(),
            progress: ProgressState::default(),
            log: LogState::default(),
            focus: FocusPanel::Devices,
            show_help: false,
            input_mode: false,
            input_buffer: String::new(),
            flash_start_time: None,
            packer: None,
            should_quit: false,
        }
    }

    pub fn is_flashing(&self) -> bool {
        self.state == AppState::Flashing
    }

    pub fn can_flash(&self) -> bool {
        !self.devices.is_empty() && self.firmware.path.is_some() && !self.is_flashing()
    }

    fn update_state(&mut self) {
        if self.state == AppState::Flashing {
            return;
        }
        if self.state == AppState::Done || self.state == AppState::Error {
            if self.can_flash() {
                self.state = AppState::Ready;
            }
            return;
        }
        if !self.devices.is_empty() && self.firmware.path.is_some() {
            self.state = AppState::Ready;
        } else {
            self.state = AppState::Idle;
        }
    }

    /// Poll GlobalProgress and update TUI progress state
    fn poll_progress(&mut self) {
        let gp = global_progress();
        let snap = gp.snapshot();

        self.progress.overall_percent = snap.precise_progress;
        self.progress.stage_progress = snap.stage_progress;
        self.progress.stage_total = snap.total_bytes;
        self.progress.speed = snap.speed;

        if !snap.current_partition.is_empty() {
            self.progress.current_partition = snap.current_partition;
        }

        // Update current stage and completed stages from snapshot
        if snap.current_stage_index < snap.stages.len() {
            let current = snap.stages[snap.current_stage_index].stage_type;
            self.progress.current_stage = Some(current);
            self.progress.stage_index = snap.current_stage_index;
        }

        self.progress.completed_stages.clear();
        for stage_info in &snap.stages {
            if stage_info.completed {
                self.progress.completed_stages.push(stage_info.stage_type);
            }
        }
    }
}

/// Run the TUI application
pub async fn run() -> anyhow::Result<()> {
    // Enable TUI mode - suppresses indicatif progress bars
    set_tui_mode(true);

    // Setup terminal
    enable_raw_mode()?;
    io::stdout().execute(EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let result = run_app(&mut terminal).await;

    // Restore terminal
    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;

    // Disable TUI mode
    set_tui_mode(false);
    terminal::set_tui_log_sender(None);

    result
}

async fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> anyhow::Result<()> {
    let mut app = App::new();

    let (tx, mut rx) = mpsc::unbounded_channel::<AppEvent>();

    // Set up TUI log channel to capture log messages from flash/scan operations
    let (log_tx, mut log_rx) = mpsc::unbounded_channel::<TuiLogMessage>();
    terminal::set_tui_log_sender(Some(log_tx));

    // Welcome message
    app.log
        .push(LogLevel::Info, "Welcome to OpenixCLI Terminal".into());
    app.log
        .push(LogLevel::Info, "Press H for help, Q to quit".into());

    // Start event loop in background
    let event_tx = tx.clone();
    tokio::spawn(async move {
        super::event::event_loop(event_tx).await;
    });

    // Auto-scan on startup
    let scan_tx = tx.clone();
    tokio::spawn(async move {
        bridge::scan_devices(scan_tx).await;
    });

    loop {
        // Drain TUI log channel → app.log
        while let Ok(msg) = log_rx.try_recv() {
            let level = match msg.level {
                TuiLogLevel::Info => LogLevel::Info,
                TuiLogLevel::Success => LogLevel::Success,
                TuiLogLevel::Warn => LogLevel::Warn,
                TuiLogLevel::Error => LogLevel::Error,
                TuiLogLevel::Debug => LogLevel::Debug,
            };
            app.log.push(level, msg.message);
        }

        // Update progress during flash
        if app.state == AppState::Flashing {
            if let Some(start) = app.flash_start_time {
                app.progress.elapsed_secs = start.elapsed().as_secs();
            }
            app.poll_progress();
        }

        // Draw
        terminal.draw(|frame| {
            ui::render(frame, &mut app);
            if app.show_help {
                ui::render_help_overlay(frame);
            }
        })?;

        // Handle events
        if let Some(event) = rx.recv().await {
            match event {
                AppEvent::Key(key) => {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }

                    // Ctrl+C always quits
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && key.code == KeyCode::Char('c')
                    {
                        break;
                    }

                    // Help overlay dismissal
                    if app.show_help {
                        app.show_help = false;
                        continue;
                    }

                    // Input mode
                    if app.input_mode {
                        handle_input_key(&mut app, key.code, &tx);
                        continue;
                    }

                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => {
                            if app.is_flashing() {
                                app.log.push(
                                    LogLevel::Warn,
                                    "Flash in progress. Use Ctrl+C to abort.".into(),
                                );
                            } else {
                                break;
                            }
                        }
                        KeyCode::Char('h') => {
                            app.show_help = true;
                        }
                        // Tab / Shift+Tab: toggle focus between Devices and Options
                        KeyCode::Tab | KeyCode::BackTab => {
                            if !app.is_flashing() {
                                app.focus = app.focus.toggle();
                            }
                        }
                        KeyCode::Char('r') if !app.is_flashing() => {
                            let scan_tx = tx.clone();
                            tokio::spawn(async move {
                                bridge::scan_devices(scan_tx).await;
                            });
                        }
                        KeyCode::Char('b') if !app.is_flashing() => {
                            app.input_mode = true;
                            app.input_buffer = app.firmware.path.clone().unwrap_or_default();
                        }
                        // Shortcut keys that work regardless of focus
                        KeyCode::Char('m') if !app.is_flashing() => {
                            app.firmware.next_mode();
                            // If leaving Partition mode while on Parts field, move to Mode
                            if app.firmware.focused_field == FirmwareField::Parts
                                && !app.firmware.has_parts_field()
                            {
                                app.firmware.focused_field = FirmwareField::Mode;
                            }
                        }
                        KeyCode::Char('v') if !app.is_flashing() => {
                            app.firmware.verify = !app.firmware.verify;
                        }
                        KeyCode::Char('a') if !app.is_flashing() => {
                            if app.firmware.focused_field == FirmwareField::Parts {
                                app.firmware.toggle_all_partitions();
                            } else {
                                app.firmware.post_action = app.firmware.post_action.next();
                            }
                        }
                        // Space: toggle partition selection
                        KeyCode::Char(' ')
                            if !app.is_flashing() && app.focus == FocusPanel::Options =>
                        {
                            app.firmware.toggle_partition();
                        }
                        // Up/Down: context-dependent
                        KeyCode::Up if !app.is_flashing() => {
                            match app.focus {
                                FocusPanel::Devices => {
                                    if app.selected_device > 0 {
                                        app.selected_device -= 1;
                                        // Scroll up if cursor goes above visible area
                                        if app.selected_device < app.device_scroll_offset {
                                            app.device_scroll_offset = app.selected_device;
                                        }
                                    }
                                }
                                FocusPanel::Options => {
                                    // If on Parts field, move cursor within partition list
                                    if app.firmware.focused_field == FirmwareField::Parts {
                                        app.firmware.move_parts_cursor_up();
                                    } else {
                                        app.firmware.focused_field = app
                                            .firmware
                                            .focused_field
                                            .prev(app.firmware.has_parts_field());
                                    }
                                }
                            }
                        }
                        KeyCode::Down if !app.is_flashing() => {
                            match app.focus {
                                FocusPanel::Devices => {
                                    if app.selected_device + 1 < app.devices.len() {
                                        app.selected_device += 1;
                                        // Scroll down if cursor goes below visible area (max 5 visible)
                                        let max_visible = 5;
                                        if app.selected_device
                                            >= app.device_scroll_offset + max_visible
                                        {
                                            app.device_scroll_offset =
                                                app.selected_device + 1 - max_visible;
                                        }
                                    }
                                }
                                FocusPanel::Options => {
                                    // If on Parts field, move cursor within partition list
                                    if app.firmware.focused_field == FirmwareField::Parts {
                                        app.firmware.move_parts_cursor_down();
                                    } else {
                                        app.firmware.focused_field = app
                                            .firmware
                                            .focused_field
                                            .next(app.firmware.has_parts_field());
                                    }
                                }
                            }
                        }
                        // Left/Right: cycle option values in Options panel
                        KeyCode::Left if !app.is_flashing() && app.focus == FocusPanel::Options => {
                            app.firmware.cycle_left();
                        }
                        KeyCode::Right
                            if !app.is_flashing() && app.focus == FocusPanel::Options =>
                        {
                            app.firmware.cycle_right();
                        }
                        KeyCode::Enter if app.can_flash() => {
                            start_flash(&mut app, &tx);
                        }
                        _ => {}
                    }
                }
                AppEvent::Tick => {
                    // Triggers redraw (progress polling done above)
                }
                AppEvent::DevicesFound(devices) => {
                    app.devices = devices;
                    if app.selected_device >= app.devices.len() {
                        app.selected_device = 0;
                    }
                    app.device_scroll_offset = 0;
                    app.update_state();
                }
                AppEvent::FlashStageStart(stage) => {
                    app.progress.current_stage = Some(stage);
                    if let Some(idx) = app.progress.all_stages.iter().position(|s| *s == stage) {
                        app.progress.stage_index = idx;
                    }
                    app.log
                        .push(LogLevel::Info, format!("Stage: {}", stage.name()));
                }
                AppEvent::FlashProgress {
                    stage_progress,
                    total,
                    speed,
                } => {
                    app.progress.stage_progress = stage_progress;
                    app.progress.stage_total = total;
                    app.progress.speed = speed;
                }
                AppEvent::FlashPartitionStart(name) => {
                    app.progress.current_partition = name.clone();
                    app.log.push(LogLevel::Info, format!("Flashing: {}", name));
                }
                AppEvent::FlashStageComplete => {
                    if let Some(stage) = app.progress.current_stage {
                        if !app.progress.completed_stages.contains(&stage) {
                            app.progress.completed_stages.push(stage);
                        }
                    }
                }
                AppEvent::FlashDone => {
                    app.state = AppState::Done;
                    app.progress.finished = true;
                    app.progress.overall_percent = 100.0;
                    app.flash_start_time = None;

                    // Mark all stages as completed
                    for stage in &app.progress.all_stages {
                        if !app.progress.completed_stages.contains(stage) {
                            app.progress.completed_stages.push(*stage);
                        }
                    }
                    app.progress.current_stage = None;

                    // Reload firmware for next flash
                    if let Some(ref path) = app.firmware.path {
                        let pathbuf = std::path::PathBuf::from(path);
                        if let Ok((packer, _, _, _)) = bridge::load_firmware(&pathbuf) {
                            app.packer = Some(packer);
                        }
                    }
                }
                AppEvent::FlashError(msg) => {
                    app.state = AppState::Error;
                    app.progress.error = Some(msg);
                    app.flash_start_time = None;

                    // Reload firmware for retry
                    if let Some(ref path) = app.firmware.path {
                        let pathbuf = std::path::PathBuf::from(path);
                        if let Ok((packer, _, _, _)) = bridge::load_firmware(&pathbuf) {
                            app.packer = Some(packer);
                        }
                    }
                }
                AppEvent::LogMessage(level, msg) => {
                    app.log.push(level, msg);
                }
            }
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

fn handle_input_key(app: &mut App, key: KeyCode, tx: &mpsc::UnboundedSender<AppEvent>) {
    match key {
        KeyCode::Esc => {
            app.input_mode = false;
            app.input_buffer.clear();
        }
        KeyCode::Enter => {
            let path = app.input_buffer.clone();
            app.input_mode = false;
            app.input_buffer.clear();

            if path.is_empty() {
                return;
            }

            let pathbuf = std::path::PathBuf::from(&path);
            if !pathbuf.exists() {
                let _ = tx.send(AppEvent::LogMessage(
                    LogLevel::Error,
                    format!("File not found: {}", path),
                ));
                return;
            }

            match bridge::load_firmware(&pathbuf) {
                Ok((packer, size, num_files, partition_names)) => {
                    app.firmware.path = Some(path);
                    app.firmware.size_mb = size / (1024 * 1024);
                    app.firmware.num_files = num_files;
                    app.firmware.selected_partitions = vec![true; partition_names.len()];
                    app.firmware.all_partitions = partition_names;
                    app.firmware.parts_cursor = 0;
                    app.firmware.parts_scroll_offset = 0;
                    app.packer = Some(packer);
                    let parts_count = app.firmware.all_partitions.len();
                    let _ = tx.send(AppEvent::LogMessage(
                        LogLevel::Info,
                        format!(
                            "Firmware loaded: {} MB, {} files, {} partitions",
                            app.firmware.size_mb, app.firmware.num_files, parts_count
                        ),
                    ));
                    app.update_state();
                }
                Err(e) => {
                    let _ = tx.send(AppEvent::LogMessage(LogLevel::Error, e));
                }
            }
        }
        KeyCode::Backspace => {
            app.input_buffer.pop();
        }
        KeyCode::Char(c) => {
            app.input_buffer.push(c);
        }
        _ => {}
    }
}

fn start_flash(app: &mut App, tx: &mpsc::UnboundedSender<AppEvent>) {
    let packer = match app.packer.take() {
        Some(p) => p,
        None => {
            let _ = tx.send(AppEvent::LogMessage(
                LogLevel::Error,
                "Firmware not loaded. Press B to load.".into(),
            ));
            return;
        }
    };

    let device = &app.devices[app.selected_device];
    let bus = Some(device.bus);
    let port = Some(device.port);
    let mode = app.firmware.mode;
    let verify = app.firmware.verify;
    let partitions = app.firmware.selected_partition_names();
    let post_action = app.firmware.post_action.as_str().to_string();

    // Reset progress and state
    app.progress.reset();
    app.progress.all_stages = if device.is_fel {
        crate::process::FlashStages::for_fel_mode()
            .stages()
            .to_vec()
    } else {
        crate::process::FlashStages::for_fes_mode()
            .stages()
            .to_vec()
    };

    app.state = AppState::Flashing;
    app.flash_start_time = Some(Instant::now());

    let flash_tx = tx.clone();
    tokio::spawn(async move {
        bridge::run_flash(
            flash_tx,
            packer,
            bus,
            port,
            mode,
            verify,
            partitions,
            post_action,
        )
        .await;
    });
}
