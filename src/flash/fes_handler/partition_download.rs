use super::types::{IncrementalChecksum, PartitionDownloadInfo, ITEM_ROOTFSFAT16};
use crate::config::mbr_parser::EFEX_CRC32_VALID_FLAG;
use crate::firmware::sparse::{is_sparse_format, sparse_format_probe, SparseParser, SPARSE_HEADER_SIZE};
use crate::firmware::OpenixPacker;
use crate::utils::{FlashError, FlashResult, Logger};
use libefex::FesDataType;
use std::io::{Cursor, Read, Seek, SeekFrom};

const CHUNK_SIZE: u64 = 256 * 1024 * 1024;

struct SparseDownloadParams<'a> {
    data_offset: u64,
    data_length: u64,
    start_sector: u32,
    partition_name: &'a str,
    verify_enabled: bool,
}

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
                info.partition_name, info.data_length, info.partition_address
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
                is_sparse_format(data)
            }
            _ => false,
        };

        if is_sparse {
            self.logger.info(&format!(
                "Partition {} is in sparse format",
                info.partition_name
            ));
            self.download_sparse_partition(ctx, packer, info, verify).await?;
            written_bytes += info.data_length;
        } else {
            written_bytes = self
                .download_raw_partition(ctx, packer, info, written_bytes, verify)
                .await?;
        }

        Ok(written_bytes)
    }

    async fn download_sparse_partition(
        &mut self,
        ctx: &libefex::Context,
        packer: &mut OpenixPacker,
        info: &PartitionDownloadInfo,
        verify: bool,
    ) -> FlashResult<()> {
        let total_size = info.data_length;
        let buffer_size = 256 * 1024usize;
        let mut all_data = Vec::with_capacity(total_size as usize);

        let mut offset: u64 = 0;
        while offset < total_size {
            let chunk_size = std::cmp::min(buffer_size as u64, total_size - offset);
            let chunk_data = packer
                .get_file_data_range_by_maintype_subtype(
                    ITEM_ROOTFSFAT16,
                    &info.download_subtype,
                    offset,
                    chunk_size,
                )
                .or_else(|_| {
                    packer.get_file_data_range_by_maintype_subtype(
                        "12345678",
                        &info.download_subtype,
                        offset,
                        chunk_size,
                    )
                })
                .map_err(|e| FlashError::InvalidFirmwareFormat(e.to_string()))?;
            all_data.extend_from_slice(&chunk_data);
            offset += chunk_size;
        }

        let mut cursor = Cursor::new(all_data);

        self.download_sparse_from_reader(
            ctx,
            &mut cursor,
            &SparseDownloadParams {
                data_offset: 0,
                data_length: total_size,
                start_sector: info.partition_address as u32,
                partition_name: &info.partition_name,
                verify_enabled: verify,
            },
        )
        .await?;

        self.logger
            .stage_complete(&format!("Partition {} flashed (sparse)", info.partition_name));

        Ok(())
    }

    async fn download_sparse_from_reader<R: Read + Seek>(
        &mut self,
        ctx: &libefex::Context,
        file: &mut R,
        params: &SparseDownloadParams<'_>,
    ) -> FlashResult<()> {
        let data_offset = params.data_offset;
        let data_length = params.data_length;
        let start_sector = params.start_sector;
        let partition_name = params.partition_name;
        let verify_enabled = params.verify_enabled;

        file.seek(SeekFrom::Start(data_offset)).map_err(|e| {
            FlashError::InvalidFirmwareFormat(format!("Failed to seek file offset: {}", e))
        })?;

        let mut header_buf = vec![0u8; SPARSE_HEADER_SIZE];
        file.read_exact(&mut header_buf).map_err(|e| {
            FlashError::InvalidFirmwareFormat(format!("Failed to read sparse header: {}", e))
        })?;

        let sparse_header = sparse_format_probe(&header_buf)?;

        let blk_sz = sparse_header.blk_sz;
        let total_blks = sparse_header.total_blks;
        let total_chunks = sparse_header.total_chunks;

        self.logger.info(&format!(
            "Sparse image: block_size={}, total_blocks={}, total_chunks={}",
            blk_sz, total_blks, total_chunks
        ));

        let mut parser = SparseParser::new(blk_sz, start_sector, verify_enabled, &*self.logger);

        let buffer_size = 256 * 1024usize;
        let mut buffer = vec![0u8; buffer_size];

        let first_read_size = std::cmp::min(buffer_size, data_length as usize);
        file.seek(SeekFrom::Start(data_offset)).map_err(|e| {
            FlashError::InvalidFirmwareFormat(format!("Failed to seek file offset: {}", e))
        })?;

        let mut read_buf = vec![0u8; first_read_size];
        file.read_exact(&mut read_buf).map_err(|e| {
            FlashError::InvalidFirmwareFormat(format!("Failed to read initial data: {}", e))
        })?;

        parser
            .parse_and_download(ctx, &read_buf, first_read_size, partition_name, data_length)
            .await?;

        let mut left_len = data_length as i64 - first_read_size as i64;

        while left_len >= buffer_size as i64 {
            file.read_exact(&mut buffer).map_err(|e| {
                FlashError::InvalidFirmwareFormat(format!("Failed to read data chunk: {}", e))
            })?;

            parser
                .parse_and_download(ctx, &buffer, buffer_size, partition_name, data_length)
                .await?;

            left_len -= buffer_size as i64;
        }

        if left_len > 0 {
            let remaining = left_len as usize;
            let mut remaining_buf = vec![0u8; remaining];
            file.read_exact(&mut remaining_buf).map_err(|e| {
                FlashError::InvalidFirmwareFormat(format!("Failed to read remaining data: {}", e))
            })?;

            parser
                .parse_and_download(ctx, &remaining_buf, remaining, partition_name, data_length)
                .await?;
        }

        if parser.need_verify() {
            self.logger.info(&format!(
                "Verifying final chunk for partition {}",
                partition_name
            ));

            let (sector, size) = parser.rawdata_info();
            let local_checksum = parser.checksum();

            let verify_resp = ctx
                .fes_verify_value(sector, size)
                .map_err(|e| FlashError::UsbTransferError(e.to_string()))?;

            if verify_resp.flag == EFEX_CRC32_VALID_FLAG {
                let device_crc = verify_resp.media_crc as u32;
                if local_checksum != device_crc {
                    self.logger.warn(&format!(
                        "Partition {} checksum mismatch: local=0x{:08x}, device=0x{:08x}",
                        partition_name, local_checksum, device_crc
                    ));
                } else {
                    self.logger.info(&format!("Partition {} verification passed", partition_name));
                }
            } else {
                self.logger.warn(&format!(
                    "Partition {} verification failed: invalid CRC flag",
                    partition_name
                ));
            }
        }

        let total_written = parser.total_written();

        self.logger.info(&format!(
            "Sparse partition {} download completed, {} bytes written",
            partition_name, total_written
        ));

        Ok(())
    }

    async fn download_raw_partition(
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

            ctx.fes_down_with_progress(&chunk_data, chunk_start_sector, FesDataType::Flash, {
                let logger = &*self.logger;
                move |transferred, _total| {
                    let current = chunk_base_bytes + transferred as usize;
                    logger.progress_update(current, &partition_name);
                }
            })
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
