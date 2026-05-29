//! Flash command implementation

use crate::commands::FlashArgs;
use crate::firmware::LoadedFirmware;
use crate::flash::Flasher;
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

    let request = args.request();

    let mut flasher = Flasher::new(loaded.into_packer(), request, logger.clone());
    if let Err(e) = flasher.execute().await {
        logger.error(&format!("Flash failed: {}", e));
        return Err(anyhow::anyhow!("{}", e));
    }

    println!();
    logger.stage_complete("All partitions flashed successfully");

    Ok(())
}
