use super::super::constants;
use super::super::types::{IncrementalChecksum, PartitionDownloadInfo, ITEM_ROOTFSFAT16};
use crate::config::mbr_parser::EFEX_CRC32_VALID_FLAG;
use crate::firmware::OpenixPacker;
use crate::utils::{FlashError, FlashResult, Logger};
use libefex::FesDataType;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

pub struct RawDownloader<'a> {
    logger: &'a Logger,
    written_bytes: Arc<AtomicU64>,
    last_speed_update: Arc<AtomicU64>,
}

impl<'a> RawDownloader<'a> {
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

    pub async fn execute(
        &self,
        ctx: &libefex::Context,
        packer: &mut OpenixPacker,
        info: &PartitionDownloadInfo,
        verify: bool,
    ) -> FlashResult<()> {
        let start_sector = info.partition_address as u32;
        let total_chunks = info.data_length.div_ceil(constants::CHUNK_SIZE);
        let mut checksum = if verify {
            Some(IncrementalChecksum::new())
        } else {
            None
        };

        for chunk_index in 0..total_chunks {
            let chunk_offset = (chunk_index * constants::CHUNK_SIZE) as usize;
            let chunk_size = std::cmp::min(
                constants::CHUNK_SIZE as usize,
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
            let written_bytes = Arc::clone(&self.written_bytes);
            let last_speed_update = Arc::clone(&self.last_speed_update);

            ctx.fes_down_with_progress(&chunk_data, chunk_start_sector, FesDataType::Flash, {
                let logger = self.logger;
                move |transferred, _total| {
                    let current =
                        written_bytes.fetch_add(transferred, Ordering::SeqCst) + transferred;
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
