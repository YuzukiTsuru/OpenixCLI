use super::super::types::{PartitionDownloadInfo, ITEM_ROOTFSFAT16};
use super::raw_download::RawDownloader;
use super::sparse_parser::SparseDownloader;
use crate::firmware::sparse::SPARSE_HEADER_SIZE;
use crate::firmware::OpenixPacker;
use crate::utils::{FlashError, FlashResult, Logger};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

pub struct PartitionDownload<'a> {
    logger: &'a mut Logger,
    written_bytes: Arc<AtomicU64>,
    last_speed_update: Arc<AtomicU64>,
}

impl<'a> PartitionDownload<'a> {
    pub fn new(logger: &'a mut Logger) -> Self {
        Self {
            logger,
            written_bytes: Arc::new(AtomicU64::new(0)),
            last_speed_update: Arc::new(AtomicU64::new(0)),
        }
    }

    pub async fn execute(
        &mut self,
        ctx: &libefex::Context,
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

        for info in download_list {
            self.logger.info(&format!(
                "Flashing partition: {} ({} bytes at sector {})",
                info.partition_name, info.data_length, info.partition_address
            ));

            self.download_single_partition(ctx, packer, info, verify)
                .await?;
        }

        self.logger.info("Turning off flash access...");
        if let Err(e) = ctx.fes_flash_set_onoff(0, false) {
            self.logger
                .warn(&format!("Failed to turn off flash access: {}", e));
        }

        let written = self.written_bytes.load(Ordering::SeqCst);
        self.logger.stage_complete(&format!(
            "All partitions flashed ({} bytes written)",
            written
        ));
        Ok(())
    }

    async fn download_single_partition(
        &mut self,
        ctx: &libefex::Context,
        packer: &mut OpenixPacker,
        info: &PartitionDownloadInfo,
        verify: bool,
    ) -> FlashResult<()> {
        self.logger.set_current_partition(&info.partition_name);
        self.last_speed_update.store(0, Ordering::SeqCst);

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

        let is_sparse = match probe_data {
            Ok(ref data) if data.len() >= SPARSE_HEADER_SIZE => {
                crate::firmware::sparse::is_sparse_format(data)
            }
            _ => false,
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

    async fn download_sparse_partition(
        &mut self,
        ctx: &libefex::Context,
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

    async fn download_raw_partition(
        &mut self,
        ctx: &libefex::Context,
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
