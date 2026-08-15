//! Raw disk image flash command.

use crate::flash::Flasher;
use crate::raw::{self, RawOptions};
use crate::utils::logger::Logger;

pub async fn execute(options: RawOptions) -> anyhow::Result<()> {
    let logger = Logger::with_verbose(options.verbose);
    logger.info(&format!(
        "Loading boot firmware: {}",
        options.firmware_path.display()
    ));
    logger.info(&format!(
        "Loading raw image: {}",
        options.image_path.display()
    ));

    let (packer, request) = raw::prepare(&options).map_err(anyhow::Error::msg)?;
    let mut flasher = Flasher::new(packer, request, logger.clone());
    if let Err(error) = flasher.execute().await {
        logger.error(&format!("Raw flash failed: {error}"));
        return Err(anyhow::anyhow!(error.to_string()));
    }

    println!();
    logger.stage_complete("Raw image flashed successfully");
    Ok(())
}
