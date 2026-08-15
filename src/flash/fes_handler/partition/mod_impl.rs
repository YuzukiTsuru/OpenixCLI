//! Partition download implementation
//!
//! Handles the main partition download logic, including format detection
//! and delegation to appropriate downloaders (raw or sparse)

use super::super::types::{PartitionDownloadInfo, PartitionSource, ITEM_ROOTFSFAT16};
use super::raw_download::RawDownloader;
use super::sparse_parser::SparseDownloader;
use crate::firmware::sparse::SPARSE_HEADER_SIZE;
use crate::firmware::OpenixPacker;
use crate::flash::protocol::FesOps;
use crate::utils::{FlashError, FlashResult, Logger};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Partition download handler
///
/// Coordinates downloading of all partitions, automatically detecting
/// whether each partition uses raw or sparse format
pub struct PartitionDownload<'a> {
    logger: &'a mut Logger,
    written_bytes: Arc<AtomicU64>,
    last_speed_update: Arc<AtomicU64>,
}

impl<'a> PartitionDownload<'a> {
    /// Create a new partition download handler
    pub fn new(logger: &'a mut Logger) -> Self {
        Self {
            logger,
            written_bytes: Arc::new(AtomicU64::new(0)),
            last_speed_update: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Execute partition download
    ///
    /// Downloads all partitions in the download list
    pub async fn execute<C: FesOps>(
        &mut self,
        ctx: &C,
        packer: &mut OpenixPacker,
        download_list: &[PartitionDownloadInfo],
        verify: bool,
    ) -> FlashResult<()> {
        if download_list.is_empty() {
            self.logger.warn("No partitions to download");
            self.logger
                .stage_complete("All partitions flashed (0 bytes written)");
            return Ok(());
        }

        self.logger
            .info(&format!("Flashing {} partitions...", download_list.len()));

        self.logger.info("Turning on flash access...");
        ctx.fes_flash_set_onoff(0, true)
            .map_err(|e| FlashError::UsbTransferError(e.to_string()))?;

        self.written_bytes.store(0, Ordering::SeqCst);
        self.last_speed_update.store(0, Ordering::SeqCst);

        let mut download_result = Ok(());
        for info in download_list {
            self.logger.info(&format!(
                "Flashing partition: {} ({} bytes at sector {})",
                info.partition_name, info.data_length, info.partition_address
            ));

            if let Err(error) = self
                .download_single_partition(ctx, packer, info, verify)
                .await
            {
                download_result = Err(error);
                break;
            }
        }

        self.logger.info("Turning off flash access...");
        if let Err(e) = ctx.fes_flash_set_onoff(0, false) {
            self.logger
                .warn(&format!("Failed to turn off flash access: {}", e));
        }

        download_result?;

        let written = self.written_bytes.load(Ordering::SeqCst);
        self.logger.stage_complete(&format!(
            "All partitions flashed ({} bytes written)",
            written
        ));
        Ok(())
    }

    /// Download a single partition
    ///
    /// Detects whether the partition is in sparse or raw format
    async fn download_single_partition<C: FesOps>(
        &mut self,
        ctx: &C,
        packer: &mut OpenixPacker,
        info: &PartitionDownloadInfo,
        verify: bool,
    ) -> FlashResult<()> {
        self.logger.set_current_partition(&info.partition_name);
        self.last_speed_update.store(0, Ordering::SeqCst);

        let is_sparse = match &info.source {
            PartitionSource::ExternalFile(_) => false,
            PartitionSource::Firmware => {
                let probe_data = packer
                    .get_file_data_range_by_maintype_subtype(
                        ITEM_ROOTFSFAT16,
                        &info.download_subtype,
                        0,
                        SPARSE_HEADER_SIZE as u64,
                    )
                    .or_else(|_| {
                        packer.get_file_data_range_by_maintype_subtype(
                            "12345678",
                            &info.download_subtype,
                            0,
                            SPARSE_HEADER_SIZE as u64,
                        )
                    });
                match probe_data {
                    Ok(ref data) if data.len() >= SPARSE_HEADER_SIZE => {
                        crate::firmware::sparse::is_sparse_format(data)
                    }
                    _ => false,
                }
            }
        };

        if is_sparse {
            self.logger.info(&format!(
                "Partition {} is in sparse format",
                info.partition_name
            ));
            self.download_sparse_partition(ctx, packer, info, verify)
                .await?;
        } else {
            self.download_raw_partition(ctx, packer, info, verify)
                .await?;
        }

        Ok(())
    }

    /// Download partition in sparse format
    async fn download_sparse_partition<C: FesOps>(
        &mut self,
        ctx: &C,
        packer: &mut OpenixPacker,
        info: &PartitionDownloadInfo,
        verify: bool,
    ) -> FlashResult<()> {
        let downloader = SparseDownloader::new(
            self.logger,
            Arc::clone(&self.written_bytes),
            Arc::clone(&self.last_speed_update),
        );
        downloader.execute(ctx, packer, info, verify).await
    }

    /// Download partition in raw format
    async fn download_raw_partition<C: FesOps>(
        &mut self,
        ctx: &C,
        packer: &mut OpenixPacker,
        info: &PartitionDownloadInfo,
        verify: bool,
    ) -> FlashResult<()> {
        let downloader = RawDownloader::new(
            self.logger,
            Arc::clone(&self.written_bytes),
            Arc::clone(&self.last_speed_update),
        );
        downloader.execute(ctx, packer, info, verify).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flash::fes_handler::types::PartitionSource;
    use crate::flash::protocol::tests::MockProtocol;
    use crate::test_support::{temp_file, test_firmware, FirmwareEntry};

    fn info(length: u64) -> PartitionDownloadInfo {
        PartitionDownloadInfo {
            partition_name: "system".to_string(),
            partition_address: 0x20,
            download_filename: "system.img".to_string(),
            download_subtype: "SYSTEM0000000000".to_string(),
            data_offset: 0,
            data_length: length,
            source: PartitionSource::Firmware,
            wrap_address: false,
        }
    }

    #[tokio::test]
    async fn empty_list_does_not_open_flash_access() {
        let firmware = test_firmware(&[]);
        let mut packer = OpenixPacker::new();
        packer.load(firmware.path()).unwrap();
        let mut logger = Logger::for_events(false, crate::flash::FlashEventSink::none());
        let ctx = MockProtocol::default();
        PartitionDownload::new(&mut logger)
            .execute(&ctx, &mut packer, &[], false)
            .await
            .unwrap();
        assert!(ctx.flash_switches.borrow().is_empty());
    }

    #[tokio::test]
    async fn successful_raw_download_opens_and_closes_flash_access() {
        let firmware = test_firmware(&[FirmwareEntry {
            filename: "system.img",
            maintype: ITEM_ROOTFSFAT16,
            subtype: "SYSTEM0000000000",
            data: b"raw",
        }]);
        let mut packer = OpenixPacker::new();
        packer.load(firmware.path()).unwrap();
        let mut logger = Logger::for_events(false, crate::flash::FlashEventSink::none());
        let ctx = MockProtocol::default();
        PartitionDownload::new(&mut logger)
            .execute(&ctx, &mut packer, &[info(3)], false)
            .await
            .unwrap();
        assert_eq!(&*ctx.flash_switches.borrow(), &[(0, true), (0, false)]);
        assert_eq!(ctx.downloads.borrow().len(), 1);
    }

    #[tokio::test]
    async fn external_raw_file_is_streamed_to_its_virtual_partition() {
        let raw = temp_file("external-raw", b"external payload");
        let firmware = test_firmware(&[]);
        let mut packer = OpenixPacker::new();
        packer.load(firmware.path()).unwrap();
        let mut logger = Logger::for_events(false, crate::flash::FlashEventSink::none());
        let ctx = MockProtocol::default();
        let external = PartitionDownloadInfo {
            partition_name: "raw".to_string(),
            partition_address: 0x1_0000_0000,
            download_filename: raw.path().display().to_string(),
            download_subtype: String::new(),
            data_offset: 0,
            data_length: 16,
            source: PartitionSource::ExternalFile(raw.path().to_path_buf()),
            wrap_address: true,
        };

        PartitionDownload::new(&mut logger)
            .execute(&ctx, &mut packer, &[external], false)
            .await
            .unwrap();

        assert_eq!(&*ctx.flash_switches.borrow(), &[(0, true), (0, false)]);
        let downloads = ctx.downloads.borrow();
        assert_eq!(downloads.len(), 1);
        assert_eq!(downloads[0].addr, 0);
        assert_eq!(downloads[0].data, b"external payload");
    }

    #[tokio::test]
    async fn failures_still_close_flash_access_and_open_failure_is_propagated() {
        let firmware = test_firmware(&[]);
        let mut packer = OpenixPacker::new();
        packer.load(firmware.path()).unwrap();
        let mut logger = Logger::for_events(false, crate::flash::FlashEventSink::none());

        let open_failed = MockProtocol::default();
        *open_failed.fail_flash_switch.borrow_mut() = Some("open".to_string());
        assert!(matches!(
            PartitionDownload::new(&mut logger)
                .execute(&open_failed, &mut packer, &[info(1)], false)
                .await,
            Err(FlashError::UsbTransferError(_))
        ));
        assert_eq!(&*open_failed.flash_switches.borrow(), &[(0, true)]);

        let download_failed = MockProtocol::default();
        assert!(matches!(
            PartitionDownload::new(&mut logger)
                .execute(&download_failed, &mut packer, &[info(1)], false)
                .await,
            Err(FlashError::PartitionDownloadFailed(_))
        ));
        assert_eq!(
            &*download_failed.flash_switches.borrow(),
            &[(0, true), (0, false)]
        );
    }

    #[tokio::test]
    async fn close_failure_is_non_fatal_after_successful_download() {
        let firmware = test_firmware(&[FirmwareEntry {
            filename: "system.img",
            maintype: ITEM_ROOTFSFAT16,
            subtype: "SYSTEM0000000000",
            data: b"raw",
        }]);
        let mut packer = OpenixPacker::new();
        packer.load(firmware.path()).unwrap();
        let mut logger = Logger::for_events(false, crate::flash::FlashEventSink::none());
        let ctx = MockProtocol::default();
        ctx.flash_switch_results
            .borrow_mut()
            .extend([Ok(()), Err("close".to_string())]);
        PartitionDownload::new(&mut logger)
            .execute(&ctx, &mut packer, &[info(3)], false)
            .await
            .unwrap();
        assert_eq!(&*ctx.flash_switches.borrow(), &[(0, true), (0, false)]);
    }
}
