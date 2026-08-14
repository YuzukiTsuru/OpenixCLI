//! Erase flag handler
//!
//! Handles sending erase flags to device before flashing

use crate::config::mbr_parser::EFEX_CRC32_VALID_FLAG;
use crate::flash::fes_handler::types::fes_data_type;
use crate::flash::protocol::FesOps;
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
    pub async fn execute<C: FesOps>(&self, ctx: &C, mode: FlashMode) -> FlashResult<()> {
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
    async fn verify_erase_flag<C: FesOps>(&self, ctx: &C) -> FlashResult<()> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flash::protocol::{tests::MockProtocol, VerifyResponse};

    #[tokio::test]
    async fn every_mode_sends_its_little_endian_erase_flag() {
        let logger = Logger::for_events(false, crate::flash::FlashEventSink::none());
        let handler = EraseFlag::new(&logger);
        for mode in [
            FlashMode::Partition,
            FlashMode::KeepData,
            FlashMode::PartitionErase,
            FlashMode::FullErase,
        ] {
            let ctx = MockProtocol::default();
            handler.execute(&ctx, mode).await.unwrap();
            let downloads = ctx.downloads.borrow();
            assert_eq!(downloads[0].data_type, FesDataType::Erase);
            assert_eq!(&downloads[0].data[..4], &mode.erase_flag().to_le_bytes());
            assert_eq!(downloads[0].data.len(), 16);
        }
    }

    #[tokio::test]
    async fn verification_handles_retry_bad_crc_exhaustion_and_errors() {
        let logger = Logger::for_events(false, crate::flash::FlashEventSink::none());
        let handler = EraseFlag::new(&logger);

        let retry = MockProtocol::default();
        retry.verify_statuses.borrow_mut().extend([
            Ok(VerifyResponse {
                flag: 0,
                media_crc: 0,
            }),
            Ok(MockProtocol::valid_response(0)),
        ]);
        handler.execute(&retry, FlashMode::FullErase).await.unwrap();
        assert_eq!(retry.verify_status_calls.borrow().len(), 2);

        let bad_crc = MockProtocol::default();
        bad_crc
            .verify_statuses
            .borrow_mut()
            .push_back(Ok(MockProtocol::valid_response(1)));
        handler
            .execute(&bad_crc, FlashMode::FullErase)
            .await
            .unwrap();
        assert_eq!(bad_crc.verify_status_calls.borrow().len(), 1);

        let exhausted = MockProtocol::default();
        exhausted
            .verify_statuses
            .borrow_mut()
            .extend((0..MAX_VERIFY_RETRIES).map(|_| {
                Ok(VerifyResponse {
                    flag: 0,
                    media_crc: 0,
                })
            }));
        handler
            .execute(&exhausted, FlashMode::FullErase)
            .await
            .unwrap();
        assert_eq!(
            exhausted.verify_status_calls.borrow().len(),
            MAX_VERIFY_RETRIES
        );

        let failed = MockProtocol::default();
        failed
            .verify_statuses
            .borrow_mut()
            .push_back(Err("verify".to_string()));
        assert!(matches!(
            handler.execute(&failed, FlashMode::FullErase).await,
            Err(FlashError::UsbTransferError(_))
        ));

        let down_failed = MockProtocol::default();
        *down_failed.fail_down.borrow_mut() = Some("down".to_string());
        assert!(matches!(
            handler.execute(&down_failed, FlashMode::FullErase).await,
            Err(FlashError::UsbTransferError(_))
        ));
    }
}
