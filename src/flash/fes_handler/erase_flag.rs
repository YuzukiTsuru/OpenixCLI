//! Erase flag handler
//!
//! Handles sending erase flags to device before flashing

use crate::config::mbr_parser::EFEX_CRC32_VALID_FLAG;
use crate::flash::fes_handler::types::fes_data_type;
use crate::flash::FlashMode;
use crate::utils::{FlashError, FlashResult, Logger};
use libefex::FesDataType;

const MAX_VERIFY_RETRIES: usize = 5;

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

        self.verify_erase_flag(ctx).await?;

        self.logger.stage_complete("Erase flag downloaded");
        Ok(())
    }

    /// Verify erase flag with retries
    async fn verify_erase_flag(&self, ctx: &libefex::Context) -> FlashResult<()> {
        let mut verify_success = false;

        for i in 0..MAX_VERIFY_RETRIES {
            self.logger.debug(&format!(
                "Verifying erase flag, attempt {}/{}",
                i + 1,
                MAX_VERIFY_RETRIES
            ));

            let verify_resp = ctx
                .fes_verify_status(fes_data_type::ERASE)
                .map_err(|e| FlashError::UsbTransferError(e.to_string()))?;

            if verify_resp.flag == EFEX_CRC32_VALID_FLAG {
                self.logger.debug("Got CRC32 valid flag");
                if verify_resp.media_crc == 0 {
                    self.logger.info("Erase flag verified successfully");
                    verify_success = true;
                } else {
                    self.logger.error(&format!(
                        "Erase flag verify failed: media_crc=0x{:08x}",
                        verify_resp.media_crc
                    ));
                }
                break;
            }

            self.logger.debug(&format!(
                "Verify status: 0x{:04x}, retrying...",
                verify_resp.flag
            ));
        }

        if !verify_success {
            self.logger
                .warn("Erase flag verification not confirmed, continuing...");
        }

        Ok(())
    }
}
