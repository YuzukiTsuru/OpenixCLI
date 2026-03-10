use crate::firmware::OpenixPacker;
use crate::flash::{FlashMode, FlashOptions, Flasher};
use crate::utils::logger::Logger;
use std::path::Path;

#[allow(clippy::too_many_arguments)]
pub async fn execute(
    firmware_path: &str,
    bus: Option<u8>,
    port: Option<u8>,
    verify: bool,
    mode: &str,
    partitions: Option<&str>,
    post_action: &str,
    verbose: bool,
) -> anyhow::Result<()> {
    let logger = Logger::with_verbose(verbose);

    logger.info(&format!("Loading firmware: {}", firmware_path));

    let firmware_path = Path::new(firmware_path);
    if !firmware_path.exists() {
        logger.error(&format!(
            "Firmware file not found: {}",
            firmware_path.display()
        ));
        return Err(anyhow::anyhow!("Firmware file not found"));
    }

    let mut packer = OpenixPacker::new();
    packer.load(firmware_path)?;

    let image_info = packer.get_image_info();
    logger.info(&format!(
        "Firmware size: {} MB, {} files",
        image_info.image_size / (1024 * 1024),
        image_info.num_files
    ));

    if let (Some(bus), Some(port)) = (bus, port) {
        logger.info(&format!("Selected device: Bus {}, Port {}", bus, port));
    } else {
        logger.info("No device specified, will use first available device");
    }

    let flash_mode = match mode {
        "partition" => FlashMode::Partition,
        "keep_data" => FlashMode::KeepData,
        "partition_erase" => FlashMode::PartitionErase,
        "full_erase" => FlashMode::FullErase,
        _ => {
            logger.error(&format!("Invalid flash mode: {}", mode));
            return Err(anyhow::anyhow!("Invalid flash mode"));
        }
    };

    let partition_list = partitions.map(|s| s.split(',').map(|p| p.trim().to_string()).collect());

    let options = FlashOptions {
        bus,
        port,
        verify,
        mode: flash_mode,
        partitions: partition_list,
        post_action: post_action.to_string(),
    };

    let mut flasher = Flasher::new(packer, options, logger.clone());
    if let Err(e) = flasher.execute().await {
        logger.error(&format!("Flash failed: {}", e));
        return Err(anyhow::anyhow!("{}", e));
    }

    println!();
    logger.stage_complete("All partitions flashed successfully");

    Ok(())
}
