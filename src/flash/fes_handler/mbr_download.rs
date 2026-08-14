//! MBR download handler
//!
//! Handles downloading MBR (Master Boot Record) to device storage

use crate::config::mbr_parser::{is_valid_mbr, EFEX_CRC32_VALID_FLAG};
use crate::flash::fes_handler::types::fes_data_type;
use crate::flash::protocol::FesOps;
use crate::utils::{FlashError, FlashResult, Logger};
use libefex::FesDataType;
use std::time::Duration;

/// Maximum number of verification retries
const MAX_VERIFY_RETRIES: usize = 5;

fn verify_delay() -> Duration {
    if cfg!(test) {
        Duration::ZERO
    } else {
        Duration::from_millis(100)
    }
}

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
    pub async fn execute<C: FesOps>(&self, ctx: &C, mbr_data: &[u8]) -> FlashResult<()> {
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
    async fn verify_mbr<C: FesOps>(&self, ctx: &C) -> FlashResult<()> {
        for _ in 0..MAX_VERIFY_RETRIES {
            let delay = verify_delay();
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }

            let verify_resp = ctx
                .fes_verify_status(fes_data_type::MBR)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flash::protocol::tests::MockProtocol;
    use crate::test_support::mbr_bytes;

    #[tokio::test]
    async fn execute_sends_mbr_and_retries_until_verified() {
        let logger = Logger::for_events(false, crate::flash::FlashEventSink::none());
        let handler = MbrDownload::new(&logger);
        let ctx = MockProtocol::default();
        ctx.verify_statuses.borrow_mut().extend([
            Ok(crate::flash::protocol::VerifyResponse {
                flag: 0,
                media_crc: 0,
            }),
            Ok(MockProtocol::valid_response(0)),
        ]);
        let mbr = mbr_bytes(&[]);

        handler.execute(&ctx, &mbr).await.unwrap();
        let downloads = ctx.downloads.borrow();
        assert_eq!(downloads.len(), 1);
        assert_eq!(downloads[0].data_type, FesDataType::Mbr);
        assert_eq!(downloads[0].data, mbr);
        assert_eq!(&*ctx.verify_status_calls.borrow(), &[fes_data_type::MBR; 2]);
    }

    #[tokio::test]
    async fn invalid_mbr_and_transport_failures_are_errors() {
        let logger = Logger::for_events(false, crate::flash::FlashEventSink::none());
        let handler = MbrDownload::new(&logger);
        let ctx = MockProtocol::default();
        assert!(matches!(
            handler.execute(&ctx, &[]).await,
            Err(FlashError::InvalidFirmwareFormat(_))
        ));
        assert!(ctx.downloads.borrow().is_empty());

        *ctx.fail_down.borrow_mut() = Some("down".to_string());
        assert!(matches!(
            handler.execute(&ctx, &mbr_bytes(&[])).await,
            Err(FlashError::UsbTransferError(_))
        ));

        let verify_error = MockProtocol::default();
        verify_error
            .verify_statuses
            .borrow_mut()
            .push_back(Err("verify".to_string()));
        assert!(matches!(
            handler.execute(&verify_error, &mbr_bytes(&[])).await,
            Err(FlashError::UsbTransferError(_))
        ));
    }

    #[tokio::test]
    async fn verification_exhaustion_is_non_fatal_and_bounded() {
        let logger = Logger::for_events(false, crate::flash::FlashEventSink::none());
        let handler = MbrDownload::new(&logger);
        let ctx = MockProtocol::default();
        ctx.verify_statuses
            .borrow_mut()
            .extend((0..MAX_VERIFY_RETRIES).map(|_| {
                Ok(crate::flash::protocol::VerifyResponse {
                    flag: EFEX_CRC32_VALID_FLAG,
                    media_crc: 1,
                })
            }));
        handler.execute(&ctx, &mbr_bytes(&[])).await.unwrap();
        assert_eq!(ctx.verify_status_calls.borrow().len(), MAX_VERIFY_RETRIES);
    }
}
