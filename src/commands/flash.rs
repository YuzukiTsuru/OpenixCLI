//! Flash command implementation

use crate::commands::FlashArgs;
use crate::firmware::{LoadedFirmware, OpenixPacker};
use crate::flash::{FlashRequest, Flasher};
use crate::utils::logger::Logger;

/// Execute the flash command
///
/// Loads firmware from the specified path and flashes it to the device
///
/// # Arguments
/// * `args` - Flash arguments including firmware path, device selection, and flash options
///
/// # Returns
/// Ok(()) on success, Error on failure
pub async fn execute(args: FlashArgs) -> anyhow::Result<()> {
    let logger = Logger::with_verbose(args.verbose);

    let (packer, request) = prepare_flash(&args, &logger)?;

    let mut flasher = Flasher::new(packer, request, logger.clone());
    if let Err(e) = flasher.execute().await {
        logger.error(&format!("Flash failed: {}", e));
        return Err(anyhow::anyhow!("{}", e));
    }

    println!();
    logger.stage_complete("All partitions flashed successfully");

    Ok(())
}

fn prepare_flash(
    args: &FlashArgs,
    logger: &Logger,
) -> anyhow::Result<(OpenixPacker, FlashRequest)> {
    logger.info(&format!(
        "Loading firmware: {}",
        args.firmware_path.display()
    ));

    if !args.firmware_path.exists() {
        logger.error(&format!(
            "Firmware file not found: {}",
            args.firmware_path.display()
        ));
        return Err(anyhow::anyhow!("Firmware file not found"));
    }

    let loaded = LoadedFirmware::load(&args.firmware_path)?;

    let image_info = loaded.image_info();
    logger.info(&format!(
        "Firmware size: {} MB, {} files",
        image_info.image_size / (1024 * 1024),
        image_info.num_files
    ));

    if let (Some(bus), Some(port)) = (args.bus, args.port) {
        logger.info(&format!("Selected device: Bus {}, Port {}", bus, port));
    } else {
        logger.info("No device specified, will use first available device");
    }

    Ok((loaded.into_packer(), args.request()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flash::{FlashEventSink, FlashMode, PostAction};
    use crate::test_support::{temp_file, test_firmware, FirmwareEntry};
    use crate::utils::terminal::set_tui_log_sender;
    use std::path::PathBuf;
    use tokio::sync::mpsc;

    fn args(path: PathBuf) -> FlashArgs {
        FlashArgs {
            firmware_path: path,
            bus: Some(1),
            port: Some(2),
            verify: false,
            mode: FlashMode::Partition,
            partitions: Some(vec!["boot".into()]),
            post_action: PostAction::Reboot,
            verbose: false,
        }
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn execute_rejects_missing_and_invalid_firmware_before_usb_access() {
        let _guard = crate::test_support::GLOBAL_STATE_TEST_LOCK.lock().unwrap();
        let (tx, _rx) = mpsc::unbounded_channel();
        set_tui_log_sender(Some(tx));

        let missing = args(PathBuf::from("missing-openixcli-firmware.fex"));
        assert_eq!(
            execute(missing).await.unwrap_err().to_string(),
            "Firmware file not found"
        );

        let invalid = temp_file("invalid-flash-command", b"invalid");
        assert!(execute(args(invalid.path().to_path_buf())).await.is_err());
        set_tui_log_sender(None);
    }

    #[test]
    fn prepare_flash_loads_metadata_and_preserves_request_options() {
        let firmware = test_firmware(&[FirmwareEntry {
            filename: "fes.fex",
            maintype: "FES",
            subtype: "FES_1-0000000000",
            data: b"fes",
        }]);
        let logger = Logger::for_events(false, FlashEventSink::none());
        let (mut packer, request) =
            prepare_flash(&args(firmware.path().to_path_buf()), &logger).unwrap();
        assert!(packer.get_fes().is_ok());
        assert_eq!(request.device.selected_pair(), Some((1, 2)));
        assert_eq!(request.mode, FlashMode::Partition);
        assert_eq!(request.partitions, Some(vec!["boot".to_string()]));

        let no_selector = FlashArgs {
            bus: None,
            port: None,
            ..args(firmware.path().to_path_buf())
        };
        let (_, request) = prepare_flash(&no_selector, &logger).unwrap();
        assert_eq!(request.device.selected_pair(), None);
    }
}
