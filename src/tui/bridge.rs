//! Bridge between TUI and existing flash/scan logic
//!
//! Provides functions to run scan and flash operations in background tasks,
//! sending progress events back to the TUI event loop.

use std::path::Path;
use tokio::sync::mpsc;

use crate::firmware::{LoadedFirmware, OpenixPacker};
use crate::flash::{
    DeviceSelector, FlashEvent, FlashEventSink, FlashLogLevel, FlashMode, FlashRequest, Flasher,
    PostAction,
};

use super::event::{AppEvent, DeviceInfo, LogLevel};

/// Scan for USB devices and send results back to TUI
pub async fn scan_devices(tx: mpsc::UnboundedSender<AppEvent>) {
    let _ = tx.send(AppEvent::LogMessage(
        LogLevel::Info,
        "Scanning for devices...".into(),
    ));

    match libefex::Context::scan_usb_devices() {
        Ok(devices) => {
            if devices.is_empty() {
                let _ = tx.send(AppEvent::LogMessage(
                    LogLevel::Warn,
                    "No devices found".into(),
                ));
                let _ = tx.send(AppEvent::DevicesFound(vec![]));
                return;
            }

            let mut infos = Vec::new();
            for dev in &devices {
                let mut ctx = libefex::Context::new();
                if ctx.scan_usb_device_at(dev.bus, dev.port).is_err() {
                    continue;
                }
                if ctx.usb_init().is_err() {
                    continue;
                }
                if ctx.efex_init().is_err() {
                    continue;
                }

                let mode = ctx.get_device_mode();
                let is_fel = mode == libefex::DeviceMode::Fel;
                let mode_str = match mode {
                    libefex::DeviceMode::Fel => "FEL".into(),
                    libefex::DeviceMode::Srv => "FES".into(),
                    libefex::DeviceMode::UpdateCool => "UPDATE_COOL".into(),
                    libefex::DeviceMode::UpdateHot => "UPDATE_HOT".into(),
                    libefex::DeviceMode::Null => "NULL".into(),
                    libefex::DeviceMode::Unknown(v) => format!("UNK(0x{:04x})", v),
                };

                let chip = ctx.get_device_mode_str().to_string();
                let chip_id = unsafe { (*ctx.as_ptr()).resp.id };

                infos.push(DeviceInfo {
                    bus: dev.bus,
                    port: dev.port,
                    mode: mode_str,
                    chip,
                    chip_id,
                    is_fel,
                });
            }

            let count = infos.len();
            let _ = tx.send(AppEvent::LogMessage(
                LogLevel::Info,
                format!("Found {} device(s)", count),
            ));
            let _ = tx.send(AppEvent::DevicesFound(infos));
        }
        Err(e) => {
            let _ = tx.send(AppEvent::LogMessage(
                LogLevel::Error,
                format!("Scan failed: {}", e),
            ));
            let _ = tx.send(AppEvent::DevicesFound(vec![]));
        }
    }
}

/// Load firmware file and return packer + metadata + partition names
pub fn load_firmware(path: &Path) -> Result<(OpenixPacker, u64, u32, Vec<String>), String> {
    let loaded =
        LoadedFirmware::load(path).map_err(|e| format!("Failed to load firmware: {}", e))?;

    let size = loaded.image_info().image_size as u64;
    let num_files = loaded.image_info().num_files;
    let partition_names = loaded.partition_names().to_vec();

    Ok((loaded.into_packer(), size, num_files, partition_names))
}

/// Run the flash operation in a background thread (not async spawn, because
/// libefex::Context contains raw pointers and is not Send).
#[allow(clippy::too_many_arguments)]
pub async fn run_flash(
    tx: mpsc::UnboundedSender<AppEvent>,
    packer: OpenixPacker,
    bus: Option<u8>,
    port: Option<u8>,
    mode: FlashMode,
    verify: bool,
    partitions: Option<Vec<String>>,
    post_action: PostAction,
) {
    let request = FlashRequest::new(
        DeviceSelector::new(bus, port),
        verify,
        mode,
        partitions,
        post_action,
    );

    let _ = tx.send(AppEvent::LogMessage(
        LogLevel::Info,
        "Starting flash...".into(),
    ));

    let event_tx = tx.clone();
    let event_sink = FlashEventSink::from_fn(move |event| {
        send_flash_event(&event_tx, event);
    });
    let logger = crate::utils::Logger::for_events(true, event_sink);

    // Run the flash in spawn_blocking since libefex::Context is !Send
    let result = tokio::task::spawn_blocking(move || {
        let rt = tokio::runtime::Handle::current();
        rt.block_on(async {
            let mut flasher = Flasher::new(packer, request, logger);
            flasher.execute().await
        })
    })
    .await;

    match result {
        Ok(Ok(())) => {
            let _ = tx.send(AppEvent::FlashDone);
            let _ = tx.send(AppEvent::LogMessage(
                LogLevel::Success,
                format!("Flash complete! Device will {}", post_action),
            ));
        }
        Ok(Err(e)) => {
            let msg = format!("{}", e);
            let _ = tx.send(AppEvent::FlashError(msg.clone()));
            let _ = tx.send(AppEvent::LogMessage(
                LogLevel::Error,
                format!("Flash failed: {}", msg),
            ));
        }
        Err(e) => {
            let msg = format!("Flash task panicked: {}", e);
            let _ = tx.send(AppEvent::FlashError(msg.clone()));
            let _ = tx.send(AppEvent::LogMessage(LogLevel::Error, msg));
        }
    }
}

fn send_flash_event(tx: &mpsc::UnboundedSender<AppEvent>, event: FlashEvent) {
    match event {
        FlashEvent::Log { level, message } => {
            let level = match level {
                FlashLogLevel::Info => LogLevel::Info,
                FlashLogLevel::Success => LogLevel::Success,
                FlashLogLevel::Warn => LogLevel::Warn,
                FlashLogLevel::Error => LogLevel::Error,
                FlashLogLevel::Debug => LogLevel::Debug,
            };
            let _ = tx.send(AppEvent::LogMessage(level, message));
        }
        FlashEvent::StagesDefined(stages) => {
            let _ = tx.send(AppEvent::FlashStagesDefined(stages));
        }
        FlashEvent::StageStarted(stage) => {
            let _ = tx.send(AppEvent::FlashStageStart(stage));
        }
        FlashEvent::StageCompleted(stage) => {
            let _ = tx.send(AppEvent::FlashStageComplete(stage));
        }
        FlashEvent::PartitionStageWeight(total) => {
            let _ = tx.send(AppEvent::FlashPartitionStageWeight(total));
        }
        FlashEvent::PartitionStarted(name) => {
            let _ = tx.send(AppEvent::FlashPartitionStart(name));
        }
        FlashEvent::Progress {
            overall_percent,
            stage_progress,
            total,
            speed,
        } => {
            let _ = tx.send(AppEvent::FlashProgress {
                overall_percent,
                stage_progress,
                total,
                speed,
            });
        }
        FlashEvent::Finished { .. } => {}
    }
}

/// A logger adapter that sends messages to the TUI (reserved for future use)
#[allow(dead_code)]
struct TuiLogger {
    tx: mpsc::UnboundedSender<AppEvent>,
}

#[allow(dead_code)]
impl TuiLogger {
    fn new(tx: mpsc::UnboundedSender<AppEvent>) -> Self {
        Self { tx }
    }
}
