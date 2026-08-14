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
    /// Flash stages were defined
    FlashStagesDefined(Vec<StageType>),
    /// Flash progress update
    FlashProgress {
        overall_percent: f64,
        stage_progress: u64,
        total: u64,
        speed: f64,
    },
    /// Total bytes for partition stage
    FlashPartitionStageWeight(u64),
    /// Flash partition started
    FlashPartitionStart(String),
    /// Flash stage completed
    FlashStageComplete(StageType),
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
                    Event::Key(key) if tx.send(AppEvent::Key(key)).is_err() => {
                        return;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_data_types_preserve_device_and_log_fields() {
        for level in [
            LogLevel::Info,
            LogLevel::Success,
            LogLevel::Warn,
            LogLevel::Error,
            LogLevel::Debug,
        ] {
            assert_eq!(level.clone(), level);
        }

        let device = DeviceInfo {
            bus: 1,
            port: 2,
            mode: "FEL".into(),
            chip: "D1".into(),
            chip_id: 0x1840,
            is_fel: true,
        };
        assert_eq!(device.bus, 1);
        assert_eq!(device.port, 2);
        assert_eq!(device.mode, "FEL");
        assert_eq!(device.chip, "D1");
        assert_eq!(device.chip_id, 0x1840);
        assert!(device.is_fel);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn event_loop_exits_when_receiver_is_closed() {
        let (tx, rx) = mpsc::unbounded_channel();
        drop(rx);
        tokio::time::timeout(Duration::from_secs(1), event_loop(tx))
            .await
            .expect("closed event loop should exit");
    }
}
