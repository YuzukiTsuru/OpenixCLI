//! Raw partition downloader
//!
//! Handles downloading raw (non-sparse) partition data to device storage

use super::super::constants;
use super::super::types::{IncrementalChecksum, PartitionDownloadInfo, ITEM_ROOTFSFAT16};
use crate::config::mbr_parser::EFEX_CRC32_VALID_FLAG;
use crate::firmware::OpenixPacker;
use crate::flash::protocol::FesOps;
use crate::utils::{FlashError, FlashResult, Logger};
use libefex::FesDataType;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Raw partition downloader
///
/// Downloads raw partition data in chunks with progress reporting
/// and optional checksum verification
pub struct RawDownloader<'a> {
    logger: &'a Logger,
    written_bytes: Arc<AtomicU64>,
    last_speed_update: Arc<AtomicU64>,
}

impl<'a> RawDownloader<'a> {
    /// Create a new raw downloader
    pub fn new(
        logger: &'a Logger,
        written_bytes: Arc<AtomicU64>,
        last_speed_update: Arc<AtomicU64>,
    ) -> Self {
        Self {
            logger,
            written_bytes,
            last_speed_update,
        }
    }

    /// Execute raw partition download
    ///
    /// Downloads partition data in chunks with progress tracking
    pub async fn execute<C: FesOps>(
        &self,
        ctx: &C,
        packer: &mut OpenixPacker,
        info: &PartitionDownloadInfo,
        verify: bool,
    ) -> FlashResult<()> {
        let start_sector = u32::try_from(info.partition_address).map_err(|_| {
            FlashError::InvalidFirmwareFormat(format!(
                "Partition {} address exceeds FES sector range: {}",
                info.partition_name, info.partition_address
            ))
        })?;
        if info.data_length == 0 {
            self.logger
                .stage_complete(&format!("Partition {} is empty", info.partition_name));
            return Ok(());
        }
        let total_chunks = info.data_length.div_ceil(constants::CHUNK_SIZE);
        let mut checksum = if verify {
            Some(IncrementalChecksum::new())
        } else {
            None
        };

        for chunk_index in 0..total_chunks {
            let chunk_offset = chunk_index * constants::CHUNK_SIZE;
            let chunk_size = std::cmp::min(
                constants::CHUNK_SIZE,
                info.data_length.saturating_sub(chunk_offset),
            );

            let chunk_data = packer
                .get_file_data_range_by_maintype_subtype(
                    ITEM_ROOTFSFAT16,
                    &info.download_subtype,
                    chunk_offset,
                    chunk_size,
                )
                .or_else(|_| {
                    packer.get_file_data_range_by_maintype_subtype(
                        "12345678",
                        &info.download_subtype,
                        chunk_offset,
                        chunk_size,
                    )
                })
                .map_err(|error| {
                    FlashError::PartitionDownloadFailed(format!(
                        "Failed to read {} at offset {}: {}",
                        info.partition_name, chunk_offset, error
                    ))
                })?;

            if let Some(ref mut cs) = checksum {
                cs.update(&chunk_data);
            }

            let sector_offset = u32::try_from(chunk_offset / 512).map_err(|_| {
                FlashError::InvalidFirmwareFormat(format!(
                    "Partition {} data offset exceeds FES sector range",
                    info.partition_name
                ))
            })?;
            let chunk_start_sector = start_sector.checked_add(sector_offset).ok_or_else(|| {
                FlashError::InvalidFirmwareFormat(format!(
                    "Partition {} end address exceeds FES sector range",
                    info.partition_name
                ))
            })?;
            let written_bytes = Arc::clone(&self.written_bytes);
            let last_speed_update = Arc::clone(&self.last_speed_update);
            let chunk_base_bytes = self.written_bytes.load(Ordering::SeqCst);

            ctx.fes_down_with_progress(&chunk_data, chunk_start_sector, FesDataType::Flash, {
                let logger = self.logger;
                move |transferred, _total| {
                    let current = chunk_base_bytes + transferred;
                    written_bytes.store(current, Ordering::SeqCst);
                    let last = last_speed_update.load(Ordering::SeqCst);

                    if current.saturating_sub(last) >= constants::SPEED_UPDATE_INTERVAL {
                        last_speed_update.store(current, Ordering::SeqCst);
                        logger.update_progress_with_speed(current);
                    }
                }
            })
            .map_err(|e| FlashError::UsbTransferError(e.to_string()))?;
        }

        self.verify_partition(ctx, info, &mut checksum).await?;

        Ok(())
    }

    /// Verify partition after download
    async fn verify_partition<C: FesOps>(
        &self,
        ctx: &C,
        info: &PartitionDownloadInfo,
        checksum: &mut Option<IncrementalChecksum>,
    ) -> FlashResult<()> {
        if checksum.is_some() {
            self.logger
                .info(&format!("Verifying partition {}...", info.partition_name));
            let local_checksum = checksum.as_mut().map(|cs| cs.finalize()).unwrap_or(0);

            let verify_resp = ctx
                .fes_verify_value(info.partition_address as u32, info.data_length)
                .map_err(|e| FlashError::UsbTransferError(e.to_string()))?;

            if verify_resp.flag == EFEX_CRC32_VALID_FLAG {
                let media_crc = verify_resp.media_crc as u32;
                if local_checksum != media_crc {
                    self.logger.warn(&format!(
                        "Partition {} checksum mismatch: local=0x{:x}, device=0x{:x}",
                        info.partition_name, local_checksum, media_crc
                    ));
                } else {
                    self.logger
                        .stage_complete(&format!("Partition {} verified", info.partition_name));
                }
            } else {
                self.logger.warn(&format!(
                    "Partition {} verification failed",
                    info.partition_name
                ));
            }
        } else {
            self.logger
                .stage_complete(&format!("Partition {} flashed", info.partition_name));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flash::protocol::{tests::MockProtocol, VerifyResponse};
    use crate::test_support::{test_firmware, FirmwareEntry};

    fn info(address: u64, length: u64) -> PartitionDownloadInfo {
        PartitionDownloadInfo {
            partition_name: "system".to_string(),
            partition_address: address,
            download_filename: "system.img".to_string(),
            download_subtype: "SYSTEM0000000000".to_string(),
            data_offset: 0,
            data_length: length,
        }
    }

    fn downloader(logger: &Logger) -> RawDownloader<'_> {
        RawDownloader::new(
            logger,
            Arc::new(AtomicU64::new(0)),
            Arc::new(AtomicU64::new(0)),
        )
    }

    #[tokio::test]
    async fn downloads_primary_and_fallback_entries_and_tracks_progress() {
        for maintype in [ITEM_ROOTFSFAT16, "12345678"] {
            let firmware = test_firmware(&[FirmwareEntry {
                filename: "system.img",
                maintype,
                subtype: "SYSTEM0000000000",
                data: b"payload",
            }]);
            let mut packer = OpenixPacker::new();
            packer.load(firmware.path()).unwrap();
            let logger = Logger::for_events(false, crate::flash::FlashEventSink::none());
            let written = Arc::new(AtomicU64::new(0));
            let handler =
                RawDownloader::new(&logger, Arc::clone(&written), Arc::new(AtomicU64::new(0)));
            let ctx = MockProtocol::default();
            handler
                .execute(&ctx, &mut packer, &info(8, 7), false)
                .await
                .unwrap();
            assert_eq!(written.load(Ordering::SeqCst), 7);
            let downloads = ctx.downloads.borrow();
            assert_eq!(downloads[0].addr, 8);
            assert_eq!(downloads[0].data_type, FesDataType::Flash);
            assert_eq!(downloads[0].data, b"payload");
            assert!(ctx.verify_value_calls.borrow().is_empty());
        }
    }

    #[tokio::test]
    async fn verification_covers_match_mismatch_invalid_flag_and_transport_error() {
        let payload = [1, 2, 3, 4, 5];
        let checksum = crate::firmware::sparse::add_sum(&payload, 0) as i32;
        for response in [
            VerifyResponse {
                flag: EFEX_CRC32_VALID_FLAG,
                media_crc: checksum,
            },
            VerifyResponse {
                flag: EFEX_CRC32_VALID_FLAG,
                media_crc: checksum.wrapping_add(1),
            },
            VerifyResponse {
                flag: 0,
                media_crc: 0,
            },
        ] {
            let firmware = test_firmware(&[FirmwareEntry {
                filename: "system.img",
                maintype: ITEM_ROOTFSFAT16,
                subtype: "SYSTEM0000000000",
                data: &payload,
            }]);
            let mut packer = OpenixPacker::new();
            packer.load(firmware.path()).unwrap();
            let logger = Logger::for_events(false, crate::flash::FlashEventSink::none());
            let ctx = MockProtocol::default();
            ctx.verify_values.borrow_mut().push_back(Ok(response));
            downloader(&logger)
                .execute(&ctx, &mut packer, &info(0x20, payload.len() as u64), true)
                .await
                .unwrap();
            assert_eq!(&*ctx.verify_value_calls.borrow(), &[(0x20, 5)]);
        }

        let firmware = test_firmware(&[FirmwareEntry {
            filename: "system.img",
            maintype: ITEM_ROOTFSFAT16,
            subtype: "SYSTEM0000000000",
            data: &payload,
        }]);
        let mut packer = OpenixPacker::new();
        packer.load(firmware.path()).unwrap();
        let logger = Logger::for_events(false, crate::flash::FlashEventSink::none());
        let ctx = MockProtocol::default();
        ctx.verify_values
            .borrow_mut()
            .push_back(Err("verify".to_string()));
        assert!(matches!(
            downloader(&logger)
                .execute(&ctx, &mut packer, &info(0, 5), true)
                .await,
            Err(FlashError::UsbTransferError(_))
        ));
    }

    #[tokio::test]
    async fn empty_invalid_missing_and_usb_failure_paths_are_explicit() {
        let firmware = test_firmware(&[]);
        let mut packer = OpenixPacker::new();
        packer.load(firmware.path()).unwrap();
        let logger = Logger::for_events(false, crate::flash::FlashEventSink::none());
        let handler = downloader(&logger);
        let ctx = MockProtocol::default();
        handler
            .execute(&ctx, &mut packer, &info(0, 0), true)
            .await
            .unwrap();
        assert!(ctx.downloads.borrow().is_empty());
        assert!(matches!(
            handler
                .execute(&ctx, &mut packer, &info(u64::from(u32::MAX) + 1, 1), false)
                .await,
            Err(FlashError::InvalidFirmwareFormat(_))
        ));
        assert!(matches!(
            handler.execute(&ctx, &mut packer, &info(0, 1), false).await,
            Err(FlashError::PartitionDownloadFailed(_))
        ));

        let data_firmware = test_firmware(&[FirmwareEntry {
            filename: "system.img",
            maintype: ITEM_ROOTFSFAT16,
            subtype: "SYSTEM0000000000",
            data: b"x",
        }]);
        let mut data_packer = OpenixPacker::new();
        data_packer.load(data_firmware.path()).unwrap();
        let failed = MockProtocol::default();
        *failed.fail_down.borrow_mut() = Some("usb".to_string());
        assert!(matches!(
            handler
                .execute(&failed, &mut data_packer, &info(0, 1), false)
                .await,
            Err(FlashError::UsbTransferError(_))
        ));
    }
}
