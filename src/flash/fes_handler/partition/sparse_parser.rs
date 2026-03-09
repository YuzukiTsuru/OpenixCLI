use super::super::types::PartitionDownloadInfo;
use crate::config::mbr_parser::EFEX_CRC32_VALID_FLAG;
use crate::firmware::sparse::{
    sparse_format_probe, ChunkHeader, LastChunkType, ParseState,
    CHUNK_HEADER_SIZE, CHUNK_TYPE_DONT_CARE, CHUNK_TYPE_FILL, CHUNK_TYPE_RAW, SPARSE_HEADER_SIZE,
};
use crate::firmware::OpenixPacker;
use crate::utils::{FlashError, FlashResult, Logger};
use libefex::FesDataType;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

const SPEED_UPDATE_INTERVAL: u64 = 64 * 1024;

pub struct SparseDownloader<'a> {
    logger: &'a Logger,
    written_bytes: Arc<AtomicU64>,
    last_speed_update: Arc<AtomicU64>,
}

impl<'a> SparseDownloader<'a> {
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
        let total_size = info.data_length;
        let buffer_size = 256 * 1024usize;
        let mut all_data = Vec::with_capacity(total_size as usize);

        let mut offset: u64 = 0;
        while offset < total_size {
            let chunk_size = std::cmp::min(buffer_size as u64, total_size - offset);
            let chunk_data = packer
                .get_file_data_range_by_maintype_subtype(
                    super::super::types::ITEM_ROOTFSFAT16,
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
        &self,
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

        let mut parser = SparseParser::new(
            blk_sz,
            start_sector,
            verify_enabled,
            self.logger,
            Arc::clone(&self.written_bytes),
            Arc::clone(&self.last_speed_update),
        );

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
}

struct SparseDownloadParams<'a> {
    data_offset: u64,
    data_length: u64,
    start_sector: u32,
    partition_name: &'a str,
    verify_enabled: bool,
}

struct SparseParser<'a> {
    state: ParseState,
    last_chunk_type: LastChunkType,
    block_size: u32,
    chunk_length: u32,
    flash_sector: u32,
    last_rest_size: usize,
    last_rest_data: Vec<u8>,
    rawdata_start_sector: u32,
    rawdata_size: u64,
    checksum: u32,
    verify_enabled: bool,
    total_written: u64,
    logger: &'a Logger,
    written_bytes: Arc<AtomicU64>,
    last_speed_update: Arc<AtomicU64>,
}

impl<'a> SparseParser<'a> {
    pub fn new(
        block_size: u32,
        start_sector: u32,
        verify_enabled: bool,
        logger: &'a Logger,
        written_bytes: Arc<AtomicU64>,
        last_speed_update: Arc<AtomicU64>,
    ) -> Self {
        SparseParser {
            state: ParseState::TotalHead,
            last_chunk_type: LastChunkType::Undefine,
            block_size,
            chunk_length: 0,
            flash_sector: start_sector,
            last_rest_size: 0,
            last_rest_data: Vec::new(),
            rawdata_start_sector: start_sector,
            rawdata_size: 0,
            checksum: 0,
            verify_enabled,
            total_written: 0,
            logger,
            written_bytes,
            last_speed_update,
        }
    }

    pub fn checksum(&self) -> u32 {
        self.checksum
    }

    pub fn rawdata_info(&self) -> (u32, u64) {
        (self.rawdata_start_sector, self.rawdata_size)
    }

    pub fn need_verify(&self) -> bool {
        self.verify_enabled && self.last_chunk_type == LastChunkType::Raw && self.rawdata_size > 0
    }

    pub fn total_written(&self) -> u64 {
        self.total_written
    }

    pub async fn parse_and_download(
        &mut self,
        ctx: &libefex::Context,
        buffer: &[u8],
        length: usize,
        partition_name: &str,
        partition_total_bytes: u64,
    ) -> FlashResult<()> {
        use crate::firmware::sparse::{
            ALIGNMENT_SIZE, MIN_DOWNLOAD_SIZE, SECTOR_SIZE,
        };

        let combined_data: Vec<u8>;
        let work_buffer: &[u8];
        let mut offset: usize;

        if self.last_rest_size > 0 && !self.last_rest_data.is_empty() {
            combined_data = [self.last_rest_data.as_slice(), &buffer[..length]].concat();
            work_buffer = &combined_data;
            offset = 0;
        } else {
            work_buffer = buffer;
            offset = 0;
        }

        let mut this_rest_size = self.last_rest_size + length;
        self.last_rest_size = 0;
        self.last_rest_data.clear();

        while this_rest_size > 0 {
            match self.state {
                ParseState::TotalHead => {
                    if this_rest_size < SPARSE_HEADER_SIZE {
                        self.save_rest_data(work_buffer, offset, this_rest_size);
                        return Ok(());
                    }

                    this_rest_size -= SPARSE_HEADER_SIZE;
                    offset += SPARSE_HEADER_SIZE;
                    self.state = ParseState::ChunkHead;
                }

                ParseState::ChunkHead => {
                    if this_rest_size < CHUNK_HEADER_SIZE {
                        self.save_rest_data(work_buffer, offset, this_rest_size);
                        return Ok(());
                    }

                    let chunk = ChunkHeader::parse(&work_buffer[offset..]).ok_or_else(|| {
                        FlashError::InvalidFirmwareFormat("Failed to parse chunk header".to_string())
                    })?;

                    let chunk_type = chunk.chunk_type;
                    let chunk_sz = chunk.chunk_sz;
                    let total_sz = chunk.total_sz;

                    offset += CHUNK_HEADER_SIZE;
                    this_rest_size -= CHUNK_HEADER_SIZE;

                    self.chunk_length = chunk_sz * self.block_size;

                    if self.verify_enabled
                        && self.last_chunk_type == LastChunkType::Raw
                        && chunk_type != CHUNK_TYPE_RAW
                        && self.rawdata_size > 0
                    {
                        self.logger.info(&format!(
                            "Verifying previous RAW chunk at sector 0x{:x}, size {} bytes",
                            self.rawdata_start_sector, self.rawdata_size
                        ));
                        self.verify_last_chunk(ctx).await?;
                    }

                    match chunk_type {
                        CHUNK_TYPE_RAW => {
                            if total_sz != self.chunk_length + CHUNK_HEADER_SIZE as u32 {
                                return Err(FlashError::InvalidFirmwareFormat(
                                    "Invalid RAW chunk size".to_string(),
                                ));
                            }

                            self.logger.debug(&format!(
                                "RAW chunk at sector 0x{:x}, size {} bytes",
                                self.flash_sector, self.chunk_length
                            ));

                            if self.last_chunk_type != LastChunkType::Raw {
                                self.checksum = 0;
                                self.rawdata_start_sector = self.flash_sector;
                                self.rawdata_size = 0;
                            }

                            self.logger.info(&format!(
                                "Downloading RAW chunk at sector 0x{:x}, size {} bytes",
                                self.flash_sector, self.chunk_length
                            ));

                            self.state = ParseState::ChunkData;
                            self.last_chunk_type = LastChunkType::Raw;
                        }

                        CHUNK_TYPE_FILL => {
                            if total_sz != CHUNK_HEADER_SIZE as u32 + 4 {
                                return Err(FlashError::InvalidFirmwareFormat(
                                    "Invalid FILL chunk size".to_string(),
                                ));
                            }

                            self.state = ParseState::ChunkFillData;
                            self.last_chunk_type = LastChunkType::Fill;
                        }

                        CHUNK_TYPE_DONT_CARE => {
                            if total_sz != CHUNK_HEADER_SIZE as u32 {
                                return Err(FlashError::InvalidFirmwareFormat(
                                    "Invalid DONT_CARE chunk size".to_string(),
                                ));
                            }

                            self.logger.debug(&format!(
                                "DONT_CARE chunk at sector 0x{:x}, size {} bytes",
                                self.flash_sector, self.chunk_length
                            ));

                            self.logger.info(&format!(
                                "don't care chunk at sector 0x{:x}, size {} bytes, total written {} bytes",
                                self.flash_sector, self.chunk_length, self.total_written
                            ));

                            self.flash_sector = self
                                .flash_sector
                                .wrapping_add(self.chunk_length / SECTOR_SIZE as u32);
                            self.state = ParseState::ChunkHead;
                            self.last_chunk_type = LastChunkType::DontCare;
                        }

                        _ => {
                            return Err(FlashError::InvalidFirmwareFormat(format!(
                                "Unknown chunk type: 0x{:x}",
                                chunk_type
                            )));
                        }
                    }
                }

                ParseState::ChunkData => {
                    let unenough_length = self.chunk_length.saturating_sub(this_rest_size as u32);

                    if unenough_length == 0 {
                        let data = &work_buffer[offset..offset + self.chunk_length as usize];
                        self.download_data(
                            ctx,
                            data,
                            true,
                            partition_name,
                            partition_total_bytes,
                        )?;

                        this_rest_size -= self.chunk_length as usize;
                        offset += self.chunk_length as usize;
                        self.chunk_length = 0;
                        self.state = ParseState::ChunkHead;
                    } else {
                        if this_rest_size < MIN_DOWNLOAD_SIZE {
                            self.save_rest_data(work_buffer, offset, this_rest_size);
                            return Ok(());
                        }

                        let download_size = if unenough_length < ALIGNMENT_SIZE as u32 {
                            this_rest_size + unenough_length as usize - ALIGNMENT_SIZE
                        } else {
                            this_rest_size & !(SECTOR_SIZE as usize - 1)
                        };

                        let data = &work_buffer[offset..offset + download_size];
                        self.download_data(
                            ctx,
                            data,
                            true,
                            partition_name,
                            partition_total_bytes,
                        )?;

                        offset += download_size;
                        self.chunk_length -= download_size as u32;
                        this_rest_size -= download_size;

                        self.save_rest_data(work_buffer, offset, this_rest_size);
                        return Ok(());
                    }
                }

                ParseState::ChunkFillData => {
                    if this_rest_size < 4 {
                        self.save_rest_data(work_buffer, offset, this_rest_size);
                        return Ok(());
                    }

                    let fill_value = u32::from_le_bytes([
                        work_buffer[offset],
                        work_buffer[offset + 1],
                        work_buffer[offset + 2],
                        work_buffer[offset + 3],
                    ]);

                    offset += 4;
                    this_rest_size -= 4;

                    self.logger.info(&format!(
                        "Downloading FILL chunk at sector 0x{:x}, size {} bytes, fill value 0x{:x}",
                        self.flash_sector, self.chunk_length, fill_value
                    ));

                    self.process_fill_chunk(ctx, fill_value, partition_name)?;
                    self.chunk_length = 0;
                    self.state = ParseState::ChunkHead;
                }
            }
        }

        Ok(())
    }

    fn save_rest_data(&mut self, buffer: &[u8], offset: usize, rest_size: usize) {
        self.last_rest_size = rest_size;
        if rest_size > 0 {
            self.last_rest_data = buffer[offset..offset + rest_size].to_vec();
        }
    }

    fn download_data(
        &mut self,
        ctx: &libefex::Context,
        data: &[u8],
        update_verify: bool,
        _partition_name: &str,
        _partition_total_bytes: u64,
    ) -> FlashResult<()> {
        use crate::firmware::sparse::{add_sum, SECTOR_SIZE};

        if data.is_empty() {
            return Ok(());
        }

        let sector = self.flash_sector;
        let written_bytes = Arc::clone(&self.written_bytes);
        let last_speed_update = Arc::clone(&self.last_speed_update);
        let logger = self.logger;

        let written = ctx
            .fes_down_with_progress(data, sector, FesDataType::Flash, move |written, _total| {
                let current = written_bytes.fetch_add(written, Ordering::SeqCst) + written;
                let last = last_speed_update.load(Ordering::SeqCst);
                
                if current.saturating_sub(last) >= SPEED_UPDATE_INTERVAL {
                    last_speed_update.store(current, Ordering::SeqCst);
                    logger.update_progress_with_speed(current);
                }
            })
            .map_err(|e| FlashError::UsbTransferError(e.to_string()))?;

        if update_verify {
            self.checksum = add_sum(data, self.checksum);
            self.rawdata_size += written;
        }
        self.total_written += written;
        self.flash_sector = self
            .flash_sector
            .wrapping_add((written / SECTOR_SIZE) as u32);

        Ok(())
    }

    fn process_fill_chunk(
        &mut self,
        ctx: &libefex::Context,
        fill_value: u32,
        partition_name: &str,
    ) -> FlashResult<()> {
        use crate::firmware::sparse::{MAX_FILL_COUNT, SECTOR_SIZE};

        if self.chunk_length == 0 {
            return Ok(());
        }

        if !self.chunk_length.is_multiple_of(SECTOR_SIZE as u32) {
            return Err(FlashError::InvalidFirmwareFormat(
                "Fill data is not sector aligned".to_string(),
            ));
        }

        self.logger.debug(&format!(
            "Processing FILL chunk: value=0x{:08x}, length={} bytes",
            fill_value, self.chunk_length
        ));

        let fill_size = MAX_FILL_COUNT as usize * 16;
        let mut fill_buffer: Vec<u8> = vec![0u8; fill_size];
        for chunk in fill_buffer.chunks_exact_mut(4) {
            chunk.copy_from_slice(&fill_value.to_le_bytes());
        }

        let mut remaining = self.chunk_length;

        while remaining >= MAX_FILL_COUNT * 16 {
            self.download_data(ctx, &fill_buffer, false, partition_name, 0)?;
            remaining -= MAX_FILL_COUNT * 16;
        }

        if remaining > 0 {
            let remaining_usize = remaining as usize;
            self.download_data(ctx, &fill_buffer[..remaining_usize], false, partition_name, 0)?;
        }

        Ok(())
    }

    async fn verify_last_chunk(&mut self, ctx: &libefex::Context) -> FlashResult<()> {
        if self.rawdata_size == 0 {
            return Ok(());
        }

        self.logger.debug(&format!(
            "Verifying chunk at sector 0x{:x}, size {} bytes",
            self.rawdata_start_sector, self.rawdata_size
        ));

        let sector = self.rawdata_start_sector;
        let size = self.rawdata_size;
        let local_checksum = self.checksum;

        let verify_resp = ctx
            .fes_verify_value(sector, size)
            .map_err(|e| FlashError::UsbTransferError(e.to_string()))?;

        if verify_resp.flag == EFEX_CRC32_VALID_FLAG {
            let device_crc = verify_resp.media_crc as u32;
            self.logger.debug(&format!(
                "Checksum: local=0x{:08x}, device=0x{:08x}",
                local_checksum, device_crc
            ));

            if local_checksum != device_crc {
                return Err(FlashError::InvalidFirmwareFormat(format!(
                    "Checksum mismatch: local=0x{:08x}, device=0x{:08x}",
                    local_checksum, device_crc
                )));
            }

            self.logger.debug("Verification passed");
            self.checksum = 0;
            self.rawdata_size = 0;
            Ok(())
        } else {
            Err(FlashError::InvalidFirmwareFormat(
                "Verification timeout: device did not return valid CRC".to_string(),
            ))
        }
    }
}
