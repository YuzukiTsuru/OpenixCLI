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

use crate::process::global_progress::set_tui_mode;

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
        }
    }

    pub fn is_flashing(&self) -> bool {
        self.state == AppState::Flashing
    }

    pub fn can_flash(&self) -> bool {
        self.devices.get(self.selected_device).is_some()
            && self.firmware.path.is_some()
            && self.packer.is_some()
            && !self.is_flashing()
    }

    #[cfg(test)]
    pub(super) fn set_test_packer(&mut self, packer: crate::firmware::OpenixPacker) {
        self.packer = Some(packer);
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
        if self.can_flash() {
            self.state = AppState::Ready;
        } else {
            self.state = AppState::Idle;
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

    result
}

async fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> anyhow::Result<()> {
    let mut app = App::new();

    let (tx, mut rx) = mpsc::unbounded_channel::<AppEvent>();

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

    run_app_loop(terminal, &mut app, &tx, &mut rx).await
}

async fn run_app_loop<B: Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    tx: &mpsc::UnboundedSender<AppEvent>,
    rx: &mut mpsc::UnboundedReceiver<AppEvent>,
) -> anyhow::Result<()>
where
    B::Error: Send + Sync + 'static,
{
    loop {
        // Update progress during flash
        if app.state == AppState::Flashing {
            if let Some(start) = app.flash_start_time {
                app.progress.elapsed_secs = start.elapsed().as_secs();
            }
        }

        // Draw
        terminal.draw(|frame| {
            ui::render(frame, app);
            if app.show_help {
                ui::render_help_overlay(frame);
            }
        })?;

        // Handle events
        let Some(event) = rx.recv().await else {
            break;
        };
        match event {
            AppEvent::Key(key) if !handle_key(app, key, tx) => break,
            AppEvent::Key(_) => {}
            event => apply_app_event(app, event),
        }
    }

    Ok(())
}

fn handle_key(
    app: &mut App,
    key: crossterm::event::KeyEvent,
    tx: &mpsc::UnboundedSender<AppEvent>,
) -> bool {
    if key.kind != KeyEventKind::Press {
        return true;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return false;
    }
    if app.show_help {
        app.show_help = false;
        return true;
    }
    if app.input_mode {
        handle_input_key(app, key.code, tx);
        return true;
    }

    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => {
            if app.is_flashing() {
                app.log.push(
                    LogLevel::Warn,
                    "Flash in progress. Use Ctrl+C to abort.".into(),
                );
            } else {
                return false;
            }
        }
        KeyCode::Char('h') => app.show_help = true,
        KeyCode::Tab | KeyCode::BackTab if !app.is_flashing() => {
            app.focus = app.focus.toggle();
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
        KeyCode::Char('m') if !app.is_flashing() => {
            app.firmware.next_mode();
            if app.firmware.focused_field == FirmwareField::Parts && !app.firmware.has_parts_field()
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
        KeyCode::Char(' ') if !app.is_flashing() && app.focus == FocusPanel::Options => {
            app.firmware.toggle_partition();
        }
        KeyCode::Up if !app.is_flashing() => match app.focus {
            FocusPanel::Devices => {
                if app.selected_device > 0 {
                    app.selected_device -= 1;
                    if app.selected_device < app.device_scroll_offset {
                        app.device_scroll_offset = app.selected_device;
                    }
                }
            }
            FocusPanel::Options => {
                if app.firmware.focused_field == FirmwareField::Parts {
                    app.firmware.move_parts_cursor_up();
                } else {
                    app.firmware.focused_field = app
                        .firmware
                        .focused_field
                        .prev(app.firmware.has_parts_field());
                }
            }
        },
        KeyCode::Down if !app.is_flashing() => match app.focus {
            FocusPanel::Devices => {
                if app
                    .selected_device
                    .checked_add(1)
                    .is_some_and(|next| next < app.devices.len())
                {
                    app.selected_device += 1;
                    let max_visible = 5;
                    if app.selected_device >= app.device_scroll_offset.saturating_add(max_visible) {
                        app.device_scroll_offset = app.selected_device + 1 - max_visible;
                    }
                }
            }
            FocusPanel::Options => {
                if app.firmware.focused_field == FirmwareField::Parts {
                    app.firmware.move_parts_cursor_down();
                } else {
                    app.firmware.focused_field = app
                        .firmware
                        .focused_field
                        .next(app.firmware.has_parts_field());
                }
            }
        },
        KeyCode::Left if !app.is_flashing() && app.focus == FocusPanel::Options => {
            app.firmware.cycle_left();
        }
        KeyCode::Right if !app.is_flashing() && app.focus == FocusPanel::Options => {
            app.firmware.cycle_right();
        }
        KeyCode::Enter if app.can_flash() => start_flash(app, tx),
        _ => {}
    }
    true
}

fn apply_app_event(app: &mut App, event: AppEvent) {
    match event {
        AppEvent::Tick => {}
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
            if let Some(idx) = app
                .progress
                .all_stages
                .iter()
                .position(|item| *item == stage)
            {
                app.progress.stage_index = idx;
            }
            app.log
                .push(LogLevel::Info, format!("Stage: {}", stage.name()));
        }
        AppEvent::FlashStagesDefined(stages) => {
            app.progress.all_stages = stages;
        }
        AppEvent::FlashProgress {
            overall_percent,
            stage_progress,
            total,
            speed,
        } => {
            app.progress.overall_percent = overall_percent;
            app.progress.stage_progress = stage_progress;
            app.progress.stage_total = total;
            app.progress.speed = speed;
        }
        AppEvent::FlashPartitionStageWeight(total) => {
            app.progress.stage_total = total;
            app.progress.stage_progress = 0;
        }
        AppEvent::FlashPartitionStart(name) => {
            app.progress.current_partition = name.clone();
            app.log.push(LogLevel::Info, format!("Flashing: {name}"));
        }
        AppEvent::FlashStageComplete(stage) => {
            if !app.progress.completed_stages.contains(&stage) {
                app.progress.completed_stages.push(stage);
            }
        }
        AppEvent::FlashDone => {
            app.state = AppState::Done;
            app.progress.finished = true;
            app.progress.overall_percent = 100.0;
            app.flash_start_time = None;

            for stage in &app.progress.all_stages {
                if !app.progress.completed_stages.contains(stage) {
                    app.progress.completed_stages.push(*stage);
                }
            }
            app.progress.current_stage = None;
            reload_firmware(app);
        }
        AppEvent::FlashError(message) => {
            app.state = AppState::Error;
            app.progress.error = Some(message);
            app.flash_start_time = None;
            reload_firmware(app);
        }
        AppEvent::LogMessage(level, message) => app.log.push(level, message),
        AppEvent::Key(_) => {}
    }
}

fn reload_firmware(app: &mut App) {
    if let Some(ref path) = app.firmware.path {
        let path = std::path::PathBuf::from(path);
        if let Ok((packer, _, _, _)) = bridge::load_firmware(&path) {
            app.packer = Some(packer);
        }
    }
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
    let post_action = app.firmware.post_action;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::StageType;
    use crate::test_support::{mbr_bytes, temp_file, test_firmware, FirmwareEntry};
    use ratatui::backend::TestBackend;

    fn device(is_fel: bool) -> DeviceInfo {
        DeviceInfo {
            bus: 1,
            port: 2,
            mode: if is_fel { "FEL" } else { "FES" }.into(),
            chip: "chip".into(),
            chip_id: 0x1890,
            is_fel,
        }
    }

    fn firmware_with_mbr() -> crate::test_support::TestFile {
        let mbr = mbr_bytes(&[("boot", 1, 2, false), ("system", 3, 4, false)]);
        test_firmware(&[FirmwareEntry {
            filename: "mbr.fex",
            maintype: "12345678",
            subtype: "1234567890___MBR",
            data: &mbr,
        }])
    }

    #[tokio::test]
    async fn app_loop_renders_and_exits_for_quit_or_a_closed_event_channel() {
        let backend = TestBackend::new(120, 32);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new();
        let (tx, mut rx) = mpsc::unbounded_channel();
        tx.send(AppEvent::Key(crossterm::event::KeyEvent::new(
            KeyCode::Char('q'),
            KeyModifiers::NONE,
        )))
        .unwrap();
        run_app_loop(&mut terminal, &mut app, &tx, &mut rx)
            .await
            .unwrap();
        assert!(terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .any(|cell| cell.symbol() == "O"));

        let backend = TestBackend::new(120, 32);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new();
        app.show_help = true;
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        drop(event_tx);
        let (action_tx, _action_rx) = mpsc::unbounded_channel();
        run_app_loop(&mut terminal, &mut app, &action_tx, &mut event_rx)
            .await
            .unwrap();
        let rendered: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(rendered.contains("Help"));
    }

    #[tokio::test]
    async fn app_loop_updates_elapsed_time_while_flashing() {
        let backend = TestBackend::new(120, 32);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new();
        app.state = AppState::Flashing;
        app.flash_start_time = Some(Instant::now() - std::time::Duration::from_secs(2));
        let (tx, mut rx) = mpsc::unbounded_channel();
        tx.send(AppEvent::Key(crossterm::event::KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        )))
        .unwrap();

        run_app_loop(&mut terminal, &mut app, &tx, &mut rx)
            .await
            .unwrap();
        assert!(app.progress.elapsed_secs >= 2);
    }

    #[test]
    fn defaults_focus_and_state_transitions_require_a_loaded_packer() {
        let mut app = App::new();
        assert_eq!(app.state, AppState::Idle);
        assert!(!app.is_flashing());
        assert!(!app.can_flash());
        assert_eq!(FocusPanel::Devices.toggle(), FocusPanel::Options);
        assert_eq!(FocusPanel::Options.toggle(), FocusPanel::Devices);

        app.devices.push(device(false));
        app.firmware.path = Some("firmware.fex".into());
        app.update_state();
        assert_eq!(app.state, AppState::Idle);

        let firmware = firmware_with_mbr();
        app.packer = Some(bridge::load_firmware(firmware.path()).unwrap().0);
        app.update_state();
        assert_eq!(app.state, AppState::Ready);
        assert!(app.can_flash());

        app.selected_device = 99;
        assert!(!app.can_flash());
        app.selected_device = 0;
        app.state = AppState::Flashing;
        app.update_state();
        assert_eq!(app.state, AppState::Flashing);
        app.state = AppState::Done;
        app.update_state();
        assert_eq!(app.state, AppState::Ready);
        app.state = AppState::Error;
        app.update_state();
        assert_eq!(app.state, AppState::Ready);
    }

    #[test]
    fn input_keys_cover_edit_cancel_missing_invalid_and_valid_firmware() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut app = App::new();
        app.input_mode = true;
        handle_input_key(&mut app, KeyCode::Char('固'), &tx);
        handle_input_key(&mut app, KeyCode::Backspace, &tx);
        handle_input_key(&mut app, KeyCode::Tab, &tx);
        assert!(app.input_buffer.is_empty());
        handle_input_key(&mut app, KeyCode::Esc, &tx);
        assert!(!app.input_mode);

        app.input_mode = true;
        handle_input_key(&mut app, KeyCode::Enter, &tx);
        assert!(rx.try_recv().is_err());

        app.input_mode = true;
        app.input_buffer = "missing-openixcli.fex".into();
        handle_input_key(&mut app, KeyCode::Enter, &tx);
        assert!(matches!(
            rx.try_recv().unwrap(),
            AppEvent::LogMessage(LogLevel::Error, message) if message.contains("File not found")
        ));

        let invalid = temp_file("invalid-tui-firmware", b"invalid");
        app.input_mode = true;
        app.input_buffer = invalid.path().display().to_string();
        handle_input_key(&mut app, KeyCode::Enter, &tx);
        assert!(matches!(
            rx.try_recv().unwrap(),
            AppEvent::LogMessage(LogLevel::Error, _)
        ));

        let firmware = firmware_with_mbr();
        app.devices.push(device(false));
        app.input_mode = true;
        app.input_buffer = firmware.path().display().to_string();
        handle_input_key(&mut app, KeyCode::Enter, &tx);
        assert_eq!(app.state, AppState::Ready);
        assert_eq!(app.firmware.all_partitions, ["boot", "system"]);
        assert_eq!(app.firmware.selected_partitions, [true, true]);
        assert!(app.packer.is_some());
        assert!(matches!(
            rx.try_recv().unwrap(),
            AppEvent::LogMessage(LogLevel::Info, message) if message.contains("2 partitions")
        ));
    }

    #[test]
    fn state_events_update_devices_progress_logs_completion_and_errors() {
        let firmware = firmware_with_mbr();
        let mut app = App::new();
        app.firmware.path = Some(firmware.path().display().to_string());
        app.packer = Some(bridge::load_firmware(firmware.path()).unwrap().0);
        app.selected_device = 5;
        app.device_scroll_offset = 4;
        apply_app_event(&mut app, AppEvent::DevicesFound(vec![device(false)]));
        assert_eq!(app.selected_device, 0);
        assert_eq!(app.device_scroll_offset, 0);
        assert_eq!(app.state, AppState::Ready);

        let stages = vec![
            StageType::Init,
            StageType::FesPartitions,
            StageType::FesBoot,
        ];
        apply_app_event(&mut app, AppEvent::FlashStagesDefined(stages.clone()));
        apply_app_event(
            &mut app,
            AppEvent::FlashStageStart(StageType::FesPartitions),
        );
        assert_eq!(app.progress.stage_index, 1);
        apply_app_event(&mut app, AppEvent::FlashPartitionStageWeight(1_000));
        apply_app_event(&mut app, AppEvent::FlashPartitionStart("rootfs".into()));
        apply_app_event(
            &mut app,
            AppEvent::FlashProgress {
                overall_percent: 50.0,
                stage_progress: 500,
                total: 1_000,
                speed: 25.0,
            },
        );
        apply_app_event(&mut app, AppEvent::FlashStageComplete(StageType::Init));
        apply_app_event(&mut app, AppEvent::FlashStageComplete(StageType::Init));
        assert_eq!(app.progress.completed_stages, [StageType::Init]);
        assert_eq!(app.progress.current_partition, "rootfs");
        assert_eq!(app.progress.stage_progress, 500);

        apply_app_event(&mut app, AppEvent::FlashDone);
        assert_eq!(app.state, AppState::Done);
        assert!(app.progress.finished);
        assert_eq!(app.progress.overall_percent, 100.0);
        assert_eq!(app.progress.completed_stages, stages);
        assert!(app.packer.is_some());

        app.packer = None;
        apply_app_event(&mut app, AppEvent::FlashError("usb failed".into()));
        assert_eq!(app.state, AppState::Error);
        assert_eq!(app.progress.error.as_deref(), Some("usb failed"));
        assert!(app.packer.is_some());

        let log_count = app.log.entries.len();
        apply_app_event(
            &mut app,
            AppEvent::LogMessage(LogLevel::Debug, "detail".into()),
        );
        apply_app_event(&mut app, AppEvent::Tick);
        apply_app_event(
            &mut app,
            AppEvent::Key(crossterm::event::KeyEvent::new(
                KeyCode::Char('x'),
                KeyModifiers::NONE,
            )),
        );
        assert_eq!(app.log.entries.len(), log_count + 1);
    }

    #[test]
    fn start_flash_without_a_packer_reports_an_error_without_spawning() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut app = App::new();
        app.devices.push(device(true));
        app.firmware.path = Some("firmware.fex".into());
        start_flash(&mut app, &tx);
        assert!(matches!(
            rx.try_recv().unwrap(),
            AppEvent::LogMessage(LogLevel::Error, message) if message.contains("not loaded")
        ));
        assert_eq!(app.state, AppState::Idle);
    }

    #[test]
    fn key_handler_covers_exit_overlays_navigation_and_option_shortcuts() {
        use crossterm::event::{KeyEvent, KeyEventKind};

        let (tx, _rx) = mpsc::unbounded_channel();
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
        let mut app = App::new();

        assert!(handle_key(
            &mut app,
            KeyEvent::new_with_kind(
                KeyCode::Char('h'),
                KeyModifiers::NONE,
                KeyEventKind::Release
            ),
            &tx,
        ));
        assert!(!app.show_help);
        assert!(!handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
            &tx,
        ));
        assert!(!handle_key(&mut app, key(KeyCode::Char('q')), &tx));

        assert!(handle_key(&mut app, key(KeyCode::Char('h')), &tx));
        assert!(app.show_help);
        assert!(handle_key(&mut app, key(KeyCode::Char('x')), &tx));
        assert!(!app.show_help);

        app.input_mode = true;
        assert!(handle_key(&mut app, key(KeyCode::Char('a')), &tx));
        assert_eq!(app.input_buffer, "a");
        app.input_mode = false;

        assert!(handle_key(&mut app, key(KeyCode::Tab), &tx));
        assert_eq!(app.focus, FocusPanel::Options);
        assert!(handle_key(&mut app, key(KeyCode::BackTab), &tx));
        assert_eq!(app.focus, FocusPanel::Devices);

        app.firmware.path = Some("firmware.fex".into());
        assert!(handle_key(&mut app, key(KeyCode::Char('b')), &tx));
        assert!(app.input_mode);
        assert_eq!(app.input_buffer, "firmware.fex");
        app.input_mode = false;

        app.firmware.mode = crate::flash::FlashMode::Partition;
        app.firmware.focused_field = FirmwareField::Parts;
        app.firmware.all_partitions = vec!["boot".into(), "system".into()];
        app.firmware.selected_partitions = vec![true, true];
        assert!(handle_key(&mut app, key(KeyCode::Char('a')), &tx));
        assert_eq!(app.firmware.selected_partitions, [false, false]);
        assert!(handle_key(&mut app, key(KeyCode::Char(' ')), &tx));
        assert_eq!(app.firmware.selected_partitions, [false, false]);
        app.focus = FocusPanel::Options;
        assert!(handle_key(&mut app, key(KeyCode::Char(' ')), &tx));
        assert_eq!(app.firmware.selected_partitions, [true, false]);

        assert!(handle_key(&mut app, key(KeyCode::Char('m')), &tx));
        assert_eq!(app.firmware.focused_field, FirmwareField::Mode);
        let verify = app.firmware.verify;
        assert!(handle_key(&mut app, key(KeyCode::Char('v')), &tx));
        assert_ne!(app.firmware.verify, verify);
        let action = app.firmware.post_action;
        app.firmware.focused_field = FirmwareField::PostAction;
        assert!(handle_key(&mut app, key(KeyCode::Char('a')), &tx));
        assert_ne!(app.firmware.post_action, action);

        app.firmware.focused_field = FirmwareField::Verify;
        let verify = app.firmware.verify;
        assert!(handle_key(&mut app, key(KeyCode::Left), &tx));
        assert_ne!(app.firmware.verify, verify);
        assert!(handle_key(&mut app, key(KeyCode::Right), &tx));
        assert_eq!(app.firmware.verify, verify);
        assert!(handle_key(&mut app, key(KeyCode::Up), &tx));
        assert_eq!(app.firmware.focused_field, FirmwareField::Mode);
        assert!(handle_key(&mut app, key(KeyCode::Down), &tx));
        assert_eq!(app.firmware.focused_field, FirmwareField::Verify);

        app.focus = FocusPanel::Devices;
        app.devices = (0..7).map(|_| device(false)).collect();
        for _ in 0..5 {
            assert!(handle_key(&mut app, key(KeyCode::Down), &tx));
        }
        assert_eq!(app.selected_device, 5);
        assert_eq!(app.device_scroll_offset, 1);
        app.device_scroll_offset = 5;
        assert!(handle_key(&mut app, key(KeyCode::Up), &tx));
        assert_eq!(app.selected_device, 4);
        assert_eq!(app.device_scroll_offset, 4);
        app.selected_device = usize::MAX;
        assert!(handle_key(&mut app, key(KeyCode::Down), &tx));

        app.state = AppState::Flashing;
        let logs = app.log.entries.len();
        assert!(handle_key(&mut app, key(KeyCode::Esc), &tx));
        assert_eq!(app.log.entries.len(), logs + 1);
        let focus = app.focus;
        assert!(handle_key(&mut app, key(KeyCode::Tab), &tx));
        assert_eq!(app.focus, focus);
    }

    #[tokio::test(flavor = "multi_thread")]
    #[allow(clippy::await_holding_lock)]
    async fn starting_flash_initializes_fel_and_fes_stages_before_safe_background_failure() {
        let _guard = crate::test_support::GLOBAL_STATE_TEST_LOCK.lock().unwrap();
        for is_fel in [false, true] {
            let firmware = firmware_with_mbr();
            let mut app = App::new();
            app.devices.push(device(is_fel));
            app.firmware.path = Some(firmware.path().display().to_string());
            app.packer = Some(bridge::load_firmware(firmware.path()).unwrap().0);
            app.update_state();
            let (tx, mut rx) = mpsc::unbounded_channel();

            assert!(handle_key(
                &mut app,
                crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                &tx,
            ));
            assert_eq!(app.state, AppState::Flashing);
            assert!(app.flash_start_time.is_some());
            assert_eq!(
                app.progress.all_stages,
                if is_fel {
                    crate::process::FlashStages::for_fel_mode()
                        .stages()
                        .to_vec()
                } else {
                    crate::process::FlashStages::for_fes_mode()
                        .stages()
                        .to_vec()
                }
            );

            tokio::time::timeout(std::time::Duration::from_secs(1), async {
                loop {
                    if matches!(rx.recv().await, Some(AppEvent::FlashError(_))) {
                        break;
                    }
                }
            })
            .await
            .expect("missing FES should fail before USB access");
        }
    }

    #[test]
    fn firmware_reload_is_a_noop_for_missing_and_invalid_paths() {
        let mut app = App::new();
        reload_firmware(&mut app);
        assert!(app.packer.is_none());
        app.firmware.path = Some("missing-openixcli-reload.fex".into());
        reload_firmware(&mut app);
        assert!(app.packer.is_none());
    }
}
