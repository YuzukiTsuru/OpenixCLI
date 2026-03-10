//! Event handling for the TUI
//!
//! Provides keyboard, tick, and flash progress events via channels.

use crossterm::event::{self, Event, KeyEvent};
use std::time::Duration;
use tokio::sync::mpsc;

use crate::process::StageType;

/// Log message severity level
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum LogLevel {
    Info,
    Success,
    Warn,
    Error,
    Debug,
}

/// Device information discovered during scanning
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub bus: u8,
    pub port: u8,
    pub mode: String,
    #[allow(dead_code)]
    pub chip: String,
    pub chip_id: u32,
    pub is_fel: bool,
}

/// Application events
#[derive(Debug)]
#[allow(dead_code)]
pub enum AppEvent {
    /// Keyboard input
    Key(KeyEvent),
    /// Periodic tick for UI refresh
    Tick,
    /// Flash stage started
    FlashStageStart(StageType),
    /// Flash progress update
    FlashProgress {
        stage_progress: u64,
        total: u64,
        speed: f64,
    },
    /// Flash partition started
    FlashPartitionStart(String),
    /// Flash stage completed
    FlashStageComplete,
    /// Flash operation completed successfully
    FlashDone,
    /// Flash operation failed
    FlashError(String),
    /// Devices found during scan
    DevicesFound(Vec<DeviceInfo>),
    /// Log message
    LogMessage(LogLevel, String),
}

/// Event loop that polls for keyboard events and generates ticks.
/// Flash events are sent directly from the bridge via the tx channel.
pub async fn event_loop(tx: mpsc::UnboundedSender<AppEvent>) {
    let tick_rate = Duration::from_millis(100);

    loop {
        // Poll for crossterm events with tick_rate timeout
        let has_event = tokio::task::block_in_place(|| event::poll(tick_rate).unwrap_or(false));

        if has_event {
            if let Ok(evt) = tokio::task::block_in_place(event::read) {
                match evt {
                    Event::Key(key) => {
                        if tx.send(AppEvent::Key(key)).is_err() {
                            return;
                        }
                    }
                    Event::Resize(_, _) => {
                        // ratatui handles resize automatically on next draw
                    }
                    _ => {}
                }
            }
        } else {
            // No event within tick_rate, send tick
            if tx.send(AppEvent::Tick).is_err() {
                return;
            }
        }
    }
}
