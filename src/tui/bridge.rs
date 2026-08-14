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

trait DeviceScanner {
    fn scan(&self) -> Result<Vec<DeviceInfo>, String>;
}

struct LibefexDeviceScanner;

impl DeviceScanner for LibefexDeviceScanner {
    fn scan(&self) -> Result<Vec<DeviceInfo>, String> {
        let devices = libefex::Context::scan_usb_devices().map_err(|error| error.to_string())?;
        let mut infos = Vec::new();
        for device in devices {
            let mut context = libefex::Context::new();
            if context.scan_usb_device_at(device.bus, device.port).is_err()
                || context.usb_init().is_err()
                || context.efex_init().is_err()
            {
                continue;
            }

            let mode = context.get_device_mode();
            let mode_name = match mode {
                libefex::DeviceMode::Fel => "FEL".into(),
                libefex::DeviceMode::Srv => "FES".into(),
                libefex::DeviceMode::UpdateCool => "UPDATE_COOL".into(),
                libefex::DeviceMode::UpdateHot => "UPDATE_HOT".into(),
                libefex::DeviceMode::Null => "NULL".into(),
                libefex::DeviceMode::Unknown(value) => format!("UNK(0x{value:04x})"),
            };
            infos.push(DeviceInfo {
                bus: device.bus,
                port: device.port,
                mode: mode_name,
                chip: context.get_device_mode_str().to_string(),
                chip_id: unsafe { (*context.as_ptr()).resp.id },
                is_fel: mode == libefex::DeviceMode::Fel,
            });
        }
        Ok(infos)
    }
}

/// Scan for USB devices and send results back to TUI
pub async fn scan_devices(tx: mpsc::UnboundedSender<AppEvent>) {
    scan_devices_with(tx, &LibefexDeviceScanner).await;
}

async fn scan_devices_with<S: DeviceScanner>(tx: mpsc::UnboundedSender<AppEvent>, scanner: &S) {
    let _ = tx.send(AppEvent::LogMessage(
        LogLevel::Info,
        "Scanning for devices...".into(),
    ));

    match scanner.scan() {
        Ok(devices) => {
            if devices.is_empty() {
                let _ = tx.send(AppEvent::LogMessage(
                    LogLevel::Warn,
                    "No devices found".into(),
                ));
                let _ = tx.send(AppEvent::DevicesFound(vec![]));
                return;
            }

            let count = devices.len();
            let _ = tx.send(AppEvent::LogMessage(
                LogLevel::Info,
                format!("Found {} device(s)", count),
            ));
            let _ = tx.send(AppEvent::DevicesFound(devices));
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

    let size = loaded.image_info().image_size;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::StageType;
    use crate::test_support::{mbr_bytes, temp_file, test_firmware, FirmwareEntry};

    struct MockScanner(Result<Vec<DeviceInfo>, String>);

    impl DeviceScanner for MockScanner {
        fn scan(&self) -> Result<Vec<DeviceInfo>, String> {
            self.0.clone()
        }
    }

    #[test]
    fn load_firmware_returns_metadata_partitions_and_errors() {
        let mbr = mbr_bytes(&[("boot", 1, 2, false), ("system", 3, 4, false)]);
        let firmware = test_firmware(&[FirmwareEntry {
            filename: "mbr.fex",
            maintype: "12345678",
            subtype: "1234567890___MBR",
            data: &mbr,
        }]);
        let (mut packer, size, files, partitions) = load_firmware(firmware.path()).unwrap();
        assert!(size > 0);
        assert_eq!(files, 1);
        assert_eq!(partitions, ["boot", "system"]);
        assert!(packer.get_mbr().is_ok());

        let invalid = temp_file("invalid-bridge-firmware", b"invalid");
        assert!(load_firmware(invalid.path())
            .err()
            .unwrap()
            .starts_with("Failed to load firmware:"));
    }

    #[test]
    fn flash_event_mapping_covers_every_log_and_progress_variant() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        for (source, expected) in [
            (FlashLogLevel::Info, LogLevel::Info),
            (FlashLogLevel::Success, LogLevel::Success),
            (FlashLogLevel::Warn, LogLevel::Warn),
            (FlashLogLevel::Error, LogLevel::Error),
            (FlashLogLevel::Debug, LogLevel::Debug),
        ] {
            send_flash_event(
                &tx,
                FlashEvent::Log {
                    level: source,
                    message: "message".into(),
                },
            );
            assert!(matches!(
                rx.try_recv().unwrap(),
                AppEvent::LogMessage(level, message) if level == expected && message == "message"
            ));
        }

        send_flash_event(
            &tx,
            FlashEvent::StagesDefined(vec![StageType::Init, StageType::FesBoot]),
        );
        assert!(matches!(
            rx.try_recv().unwrap(),
            AppEvent::FlashStagesDefined(stages) if stages == [StageType::Init, StageType::FesBoot]
        ));
        send_flash_event(&tx, FlashEvent::StageStarted(StageType::FesMbr));
        assert!(matches!(
            rx.try_recv().unwrap(),
            AppEvent::FlashStageStart(StageType::FesMbr)
        ));
        send_flash_event(&tx, FlashEvent::StageCompleted(StageType::FesMbr));
        assert!(matches!(
            rx.try_recv().unwrap(),
            AppEvent::FlashStageComplete(StageType::FesMbr)
        ));
        send_flash_event(&tx, FlashEvent::PartitionStageWeight(100));
        assert!(matches!(
            rx.try_recv().unwrap(),
            AppEvent::FlashPartitionStageWeight(100)
        ));
        send_flash_event(&tx, FlashEvent::PartitionStarted("rootfs".into()));
        assert!(matches!(
            rx.try_recv().unwrap(),
            AppEvent::FlashPartitionStart(name) if name == "rootfs"
        ));
        send_flash_event(
            &tx,
            FlashEvent::Progress {
                overall_percent: 12.5,
                stage_progress: 25,
                total: 200,
                speed: 3.5,
            },
        );
        assert!(matches!(
            rx.try_recv().unwrap(),
            AppEvent::FlashProgress {
                overall_percent: 12.5,
                stage_progress: 25,
                total: 200,
                speed: 3.5
            }
        ));

        send_flash_event(
            &tx,
            FlashEvent::Finished {
                post_action: PostAction::Reboot,
            },
        );
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn tui_logger_constructor_preserves_sender() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let logger = TuiLogger::new(tx.clone());
        assert!(!logger.tx.is_closed());
    }

    #[tokio::test]
    async fn device_scan_bridge_emits_start_empty_success_and_error_sequences() {
        let device = DeviceInfo {
            bus: 1,
            port: 2,
            mode: "FES".into(),
            chip: "chip".into(),
            chip_id: 0x1890,
            is_fel: false,
        };
        for scanner in [MockScanner(Ok(Vec::new())), MockScanner(Ok(vec![device]))] {
            let (tx, mut rx) = mpsc::unbounded_channel();
            scan_devices_with(tx, &scanner).await;
            assert!(matches!(
                rx.try_recv().unwrap(),
                AppEvent::LogMessage(LogLevel::Info, message) if message.contains("Scanning")
            ));
            let second = rx.try_recv().unwrap();
            match scanner.0 {
                Ok(ref devices) if devices.is_empty() => {
                    assert!(matches!(second, AppEvent::LogMessage(LogLevel::Warn, _)))
                }
                Ok(_) => assert!(matches!(second, AppEvent::LogMessage(LogLevel::Info, _))),
                Err(_) => unreachable!(),
            }
            assert!(matches!(rx.try_recv().unwrap(), AppEvent::DevicesFound(_)));
        }

        let (tx, mut rx) = mpsc::unbounded_channel();
        scan_devices_with(tx, &MockScanner(Err("usb unavailable".into()))).await;
        let _ = rx.try_recv().unwrap();
        assert!(matches!(
            rx.try_recv().unwrap(),
            AppEvent::LogMessage(LogLevel::Error, message) if message.contains("usb unavailable")
        ));
        assert!(matches!(
            rx.try_recv().unwrap(),
            AppEvent::DevicesFound(devices) if devices.is_empty()
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn run_flash_reports_missing_fes_from_background_worker() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        run_flash(
            tx,
            OpenixPacker::new(),
            None,
            None,
            FlashMode::FullErase,
            false,
            None,
            PostAction::Reboot,
        )
        .await;

        assert!(matches!(
            rx.try_recv().unwrap(),
            AppEvent::LogMessage(LogLevel::Info, message) if message.contains("Starting flash")
        ));
        assert!(matches!(
            rx.try_recv().unwrap(),
            AppEvent::FlashError(message) if message.contains("FES not found")
        ));
        assert!(matches!(
            rx.try_recv().unwrap(),
            AppEvent::LogMessage(LogLevel::Error, message) if message.contains("Flash failed")
        ));
    }
}
