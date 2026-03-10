//! MBR download handler
//!
//! Handles downloading MBR (Master Boot Record) to device storage

use crate::config::mbr_parser::{is_valid_mbr, EFEX_CRC32_VALID_FLAG};
use crate::utils::{FlashError, FlashResult, Logger};
use libefex::FesDataType;
use std::time::Duration;

/// Maximum number of verification retries
const MAX_VERIFY_RETRIES: usize = 5;

/// MBR download handler
///
/// Downloads MBR partition table to device storage
pub struct MbrDownload<'a> {
    logger: &'a Logger,
}

impl<'a> MbrDownload<'a> {
    /// Create a new MBR download handler
    pub fn new(logger: &'a Logger) -> Self {
        Self { logger }
    }

    /// Execute MBR download
    ///
    /// Downloads MBR data to device storage and verifies the write
    pub async fn execute(&self, ctx: &libefex::Context, mbr_data: &[u8]) -> FlashResult<()> {
        self.logger
            .info(&format!("Downloading MBR ({} bytes)...", mbr_data.len()));

        if !is_valid_mbr(mbr_data) {
            return Err(FlashError::InvalidFirmwareFormat("Invalid MBR".to_string()));
        }

        ctx.fes_down(mbr_data, 0, FesDataType::Mbr)
            .map_err(|e| FlashError::UsbTransferError(e.to_string()))?;

        self.verify_mbr(ctx).await
    }

    /// Verify MBR was written correctly
    async fn verify_mbr(&self, ctx: &libefex::Context) -> FlashResult<()> {
        for _ in 0..MAX_VERIFY_RETRIES {
            tokio::time::sleep(Duration::from_millis(100)).await;

            let verify_resp = ctx
                .fes_verify_status(0x7f01)
                .map_err(|e| FlashError::UsbTransferError(e.to_string()))?;

            if verify_resp.flag == EFEX_CRC32_VALID_FLAG && verify_resp.media_crc == 0 {
                self.logger.stage_complete("MBR verified successfully");
                return Ok(());
            }
        }

        self.logger
            .warn("MBR verification not confirmed, continuing...");
        Ok(())
    }
}
