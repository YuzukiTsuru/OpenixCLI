//! Bridge between TUI and existing flash/scan logic
//!
//! Provides functions to run scan and flash operations in background tasks,
//! sending progress events back to the TUI event loop.

use std::path::PathBuf;
use tokio::sync::mpsc;

use crate::commands::types::FlashMode as CmdFlashMode;
use crate::config::mbr_parser::SunxiMbr;
use crate::firmware::OpenixPacker;
use crate::flash::{FlashMode, FlashOptions, Flasher};

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
pub fn load_firmware(path: &PathBuf) -> Result<(OpenixPacker, u64, u32, Vec<String>), String> {
    let mut packer = OpenixPacker::new();
    packer
        .load(path)
        .map_err(|e| format!("Failed to load firmware: {}", e))?;

    let info = packer.get_image_info();

    // Extract partition names from MBR
    let partition_names = match packer.get_mbr() {
        Ok(mbr_data) => match SunxiMbr::parse(&mbr_data) {
            Ok(mbr) => mbr.partitions.iter().map(|p| p.name.clone()).collect(),
            Err(_) => Vec::new(),
        },
        Err(_) => Vec::new(),
    };

    Ok((
        packer,
        info.image_size as u64,
        info.num_files,
        partition_names,
    ))
}

/// Run the flash operation in a background thread (not async spawn, because
/// libefex::Context contains raw pointers and is not Send).
#[allow(clippy::too_many_arguments)]
pub async fn run_flash(
    tx: mpsc::UnboundedSender<AppEvent>,
    packer: OpenixPacker,
    bus: Option<u8>,
    port: Option<u8>,
    mode: CmdFlashMode,
    verify: bool,
    partitions: Option<Vec<String>>,
    post_action: String,
) {
    let flash_mode = match mode {
        CmdFlashMode::Partition => FlashMode::Partition,
        CmdFlashMode::KeepData => FlashMode::KeepData,
        CmdFlashMode::PartitionErase => FlashMode::PartitionErase,
        CmdFlashMode::FullErase => FlashMode::FullErase,
    };

    let options = FlashOptions {
        bus,
        port,
        verify,
        mode: flash_mode,
        partitions,
        post_action: post_action.clone(),
    };

    let _ = tx.send(AppEvent::LogMessage(
        LogLevel::Info,
        "Starting flash...".into(),
    ));

    let cli_logger = crate::utils::Logger::with_verbose(true);

    // Run the flash in spawn_blocking since libefex::Context is !Send
    let result = tokio::task::spawn_blocking(move || {
        let rt = tokio::runtime::Handle::current();
        rt.block_on(async {
            let mut flasher = Flasher::new(packer, options, cli_logger);
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
