//! Erase flag handler
//!
//! Handles sending erase flags to device before flashing

use crate::flash::FlashMode;
use crate::utils::{FlashError, FlashResult, Logger};
use libefex::FesDataType;

/// Erase flag handler
///
/// Sends erase flags to the device based on the selected flash mode
pub struct EraseFlag<'a> {
    logger: &'a Logger,
}

impl<'a> EraseFlag<'a> {
    /// Create a new erase flag handler
    pub fn new(logger: &'a Logger) -> Self {
        Self { logger }
    }

    /// Execute erase flag download
    ///
    /// Downloads the appropriate erase flag to the device based on flash mode
    pub async fn execute(&self, ctx: &libefex::Context, mode: FlashMode) -> FlashResult<()> {
        self.logger.info("Downloading erase flag...");

        let mut erase_data = vec![0u8; 16];
        let erase_flag = mode.erase_flag();
        erase_data[0..4].copy_from_slice(&erase_flag.to_le_bytes());

        ctx.fes_down(&erase_data, 0, FesDataType::Erase)
            .map_err(|e| FlashError::UsbTransferError(e.to_string()))?;

        self.logger.stage_complete("Erase flag downloaded");
        Ok(())
    }
}
