use crate::commands::FlashArgs;
use crate::flash::{FlashMode, FlashOptions, Flasher};
use crate::utils::logger::Logger;

pub async fn execute(args: FlashArgs) -> anyhow::Result<()> {
    let logger = Logger::with_verbose(args.verbose);

    logger.info(&format!("Loading firmware: {}", args.firmware_path.display()));

    if !args.firmware_path.exists() {
        logger.error(&format!(
            "Firmware file not found: {}",
            args.firmware_path.display()
        ));
        return Err(anyhow::anyhow!("Firmware file not found"));
    }

    let mut packer = crate::firmware::OpenixPacker::new();
    packer.load(&args.firmware_path)?;

    let image_info = packer.get_image_info();
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

    let options = FlashOptions {
        bus: args.bus,
        port: args.port,
        verify: args.verify,
        mode: match args.mode {
            crate::commands::FlashMode::Partition => FlashMode::Partition,
            crate::commands::FlashMode::KeepData => FlashMode::KeepData,
            crate::commands::FlashMode::PartitionErase => FlashMode::PartitionErase,
            crate::commands::FlashMode::FullErase => FlashMode::FullErase,
        },
        partitions: args.partitions,
        post_action: args.post_action,
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
