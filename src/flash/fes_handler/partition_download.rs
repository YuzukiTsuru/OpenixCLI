use super::types::{IncrementalChecksum, PartitionDownloadInfo, ITEM_ROOTFSFAT16};
use crate::config::mbr_parser::EFEX_CRC32_VALID_FLAG;
use crate::firmware::OpenixPacker;
use crate::utils::{FlashError, FlashResult, Logger};
use libefex::FesDataType;

const CHUNK_SIZE: u64 = 256 * 1024 * 1024;

pub struct PartitionDownload<'a> {
    logger: &'a mut Logger,
}

impl<'a> PartitionDownload<'a> {
    pub fn new(logger: &'a mut Logger) -> Self {
        Self { logger }
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

        let result = self
            .download_partitions_inner(ctx, packer, download_list, verify)
            .await;

        self.logger.info("Turning off flash access...");
        if let Err(e) = ctx.fes_flash_set_onoff(0, false) {
            self.logger
                .warn(&format!("Failed to turn off flash access: {}", e));
        }

        result
    }

    async fn download_partitions_inner(
        &mut self,
        ctx: &libefex::Context,
        packer: &mut OpenixPacker,
        download_list: &[PartitionDownloadInfo],
        verify: bool,
    ) -> FlashResult<()> {
        let total_bytes: u64 = download_list.iter().map(|p| p.data_length).sum();
        let mut written_bytes: u64 = 0;

        if total_bytes > 0 {
            self.logger
                .start_global_progress(total_bytes, "Initializing...");
        }

        for info in download_list {
            self.logger.info(&format!(
                "Flashing partition: {} ({} bytes at sector {})",
                info.partition_name,
                info.data_length,
                info.partition_address
            ));

            written_bytes = self
                .download_single_partition(ctx, packer, info, written_bytes, verify)
                .await?;
        }

        self.logger.stage_complete(&format!(
            "All partitions flashed ({} bytes written)",
            written_bytes
        ));
        Ok(())
    }

    async fn download_single_partition(
        &mut self,
        ctx: &libefex::Context,
        packer: &mut OpenixPacker,
        info: &PartitionDownloadInfo,
        mut written_bytes: u64,
        verify: bool,
    ) -> FlashResult<u64> {
        let start_sector = info.partition_address as u32;
        let total_chunks = info.data_length.div_ceil(CHUNK_SIZE);
        let mut checksum = if verify {
            Some(IncrementalChecksum::new())
        } else {
            None
        };

        for chunk_index in 0..total_chunks {
            let chunk_offset = (chunk_index * CHUNK_SIZE) as usize;
            let chunk_size = std::cmp::min(
                CHUNK_SIZE as usize,
                (info.data_length as usize).saturating_sub(chunk_offset),
            );

            let chunk_data = packer
                .get_file_data_range_by_maintype_subtype(
                    ITEM_ROOTFSFAT16,
                    &info.download_subtype,
                    chunk_offset as u64,
                    chunk_size as u64,
                )
                .or_else(|_| {
                    packer.get_file_data_range_by_maintype_subtype(
                        "12345678",
                        &info.download_subtype,
                        chunk_offset as u64,
                        chunk_size as u64,
                    )
                });

            let chunk_data = match chunk_data {
                Ok(data) => data,
                Err(e) => {
                    self.logger.warn(&format!(
                        "Failed to read chunk data for {}: {}",
                        info.partition_name, e
                    ));
                    break;
                }
            };

            if let Some(ref mut cs) = checksum {
                cs.update(&chunk_data);
            }

            let chunk_start_sector = start_sector.wrapping_add((chunk_offset / 512) as u32);
            let partition_name = info.partition_name.clone();
            let chunk_base_bytes = written_bytes as usize;

            ctx.fes_down_with_progress(
                &chunk_data,
                chunk_start_sector,
                FesDataType::Flash,
                {
                    let logger = &*self.logger;
                    move |transferred, _total| {
                        let current = chunk_base_bytes + transferred as usize;
                        logger.progress_update(current, &partition_name);
                    }
                },
            )
            .map_err(|e| FlashError::UsbTransferError(e.to_string()))?;

            written_bytes += chunk_size as u64;
        }

        self.verify_partition(ctx, info, &mut checksum).await?;

        Ok(written_bytes)
    }

    async fn verify_partition(
        &self,
        ctx: &libefex::Context,
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
