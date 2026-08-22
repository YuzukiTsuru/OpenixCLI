//! Sparse partition downloader
//!
//! Handles downloading sparse partition data to device storage
//! Sparse format is a compressed format used by Android system images

use super::super::constants;
use super::super::types::PartitionDownloadInfo;
use crate::config::mbr_parser::EFEX_CRC32_VALID_FLAG;
use crate::firmware::sparse::{
    sparse_format_probe, ChunkHeader, LastChunkType, ParseState, CHUNK_HEADER_SIZE,
    CHUNK_TYPE_CRC32, CHUNK_TYPE_DONT_CARE, CHUNK_TYPE_FILL, CHUNK_TYPE_RAW, SPARSE_HEADER_SIZE,
};
use crate::firmware::OpenixPacker;
use crate::flash::protocol::FesOps;
use crate::utils::{FlashError, FlashResult, Logger};
use libefex::FesDataType;
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Sparse partition downloader
///
/// Downloads sparse format partition data with chunk parsing
/// and optional checksum verification
pub struct SparseDownloader<'a> {
    logger: &'a Logger,
    written_bytes: Arc<AtomicU64>,
    last_speed_update: Arc<AtomicU64>,
}

impl<'a> SparseDownloader<'a> {
    /// Create a new sparse downloader
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

    /// Execute sparse partition download
    ///
    /// Reads partition data and downloads using sparse format parser
    pub async fn execute<C: FesOps>(
        &self,
        ctx: &C,
        packer: &mut OpenixPacker,
        info: &PartitionDownloadInfo,
        verify: bool,
    ) -> FlashResult<()> {
        let total_size = info.data_length;
        let start_sector = u32::try_from(info.partition_address).map_err(|_| {
            FlashError::InvalidFirmwareFormat(format!(
                "Partition {} address exceeds FES sector range: {}",
                info.partition_name, info.partition_address
            ))
        })?;
        let mut reader = PackerEntryReader::new(packer, &info.download_subtype, total_size);

        self.download_sparse_from_reader(
            ctx,
            &mut reader,
            &SparseDownloadParams {
                data_offset: 0,
                data_length: total_size,
                start_sector,
                partition_name: &info.partition_name,
                verify_enabled: verify,
            },
        )
        .await?;

        self.logger.stage_complete(&format!(
            "Partition {} flashed (sparse)",
            info.partition_name
        ));

        Ok(())
    }

    /// Download sparse data from a reader
    ///
    /// Parses sparse format and downloads chunks to device
    async fn download_sparse_from_reader<C: FesOps, R: Read + Seek>(
        &self,
        ctx: &C,
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

        let mut buffer = vec![0u8; constants::BUFFER_SIZE];

        let first_read_size = usize::try_from(std::cmp::min(
            constants::BUFFER_SIZE as u64,
            data_length,
        ))
        .map_err(|_| {
            FlashError::InvalidFirmwareFormat("Sparse initial read size overflow".to_string())
        })?;
        file.seek(SeekFrom::Start(data_offset)).map_err(|e| {
            FlashError::InvalidFirmwareFormat(format!("Failed to seek file offset: {}", e))
        })?;

        let mut read_buf = vec![0u8; first_read_size];
        file.read_exact(&mut read_buf).map_err(|e| {
            FlashError::InvalidFirmwareFormat(format!("Failed to read initial data: {}", e))
        })?;

        parser
            .parse_and_download(ctx, &read_buf, first_read_size)
            .await?;

        let mut consumed = first_read_size as u64;

        while data_length.saturating_sub(consumed) >= constants::BUFFER_SIZE as u64 {
            file.read_exact(&mut buffer).map_err(|e| {
                FlashError::InvalidFirmwareFormat(format!("Failed to read data chunk: {}", e))
            })?;

            parser
                .parse_and_download(ctx, &buffer, constants::BUFFER_SIZE)
                .await?;

            consumed += constants::BUFFER_SIZE as u64;
        }

        let remaining = usize::try_from(data_length.saturating_sub(consumed)).map_err(|_| {
            FlashError::InvalidFirmwareFormat("Sparse remaining size overflow".to_string())
        })?;
        if remaining > 0 {
            let mut remaining_buf = vec![0u8; remaining];
            file.read_exact(&mut remaining_buf).map_err(|e| {
                FlashError::InvalidFirmwareFormat(format!("Failed to read remaining data: {}", e))
            })?;

            parser
                .parse_and_download(ctx, &remaining_buf, remaining)
                .await?;
        }

        parser.finish(total_chunks, total_blks)?;

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
                    self.logger
                        .info(&format!("Partition {} verification passed", partition_name));
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

struct PackerEntryReader<'a> {
    packer: &'a mut OpenixPacker,
    subtype: &'a str,
    length: u64,
    position: u64,
}

impl<'a> PackerEntryReader<'a> {
    fn new(packer: &'a mut OpenixPacker, subtype: &'a str, length: u64) -> Self {
        Self {
            packer,
            subtype,
            length,
            position: 0,
        }
    }
}

impl Read for PackerEntryReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() || self.position >= self.length {
            return Ok(0);
        }
        let length = std::cmp::min(buffer.len() as u64, self.length - self.position);
        let data = self
            .packer
            .get_file_data_range_by_maintype_subtype(
                super::super::types::ITEM_ROOTFSFAT16,
                self.subtype,
                self.position,
                length,
            )
            .or_else(|_| {
                self.packer.get_file_data_range_by_maintype_subtype(
                    "12345678",
                    self.subtype,
                    self.position,
                    length,
                )
            })
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
        buffer[..data.len()].copy_from_slice(&data);
        self.position += data.len() as u64;
        Ok(data.len())
    }
}

impl Seek for PackerEntryReader<'_> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        let next = match position {
            SeekFrom::Start(value) => i128::from(value),
            SeekFrom::Current(value) => i128::from(self.position) + i128::from(value),
            SeekFrom::End(value) => i128::from(self.length) + i128::from(value),
        };
        if !(0..=i128::from(u64::MAX)).contains(&next) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid sparse entry seek",
            ));
        }
        self.position = next as u64;
        Ok(self.position)
    }
}

/// Parameters for sparse download operation
struct SparseDownloadParams<'a> {
    data_offset: u64,
    data_length: u64,
    start_sector: u32,
    partition_name: &'a str,
    verify_enabled: bool,
}

/// Sparse format parser
///
/// Parses sparse image format chunks and downloads them to device
struct SparseParser<'a> {
    state: ParseState,
    last_chunk_type: LastChunkType,
    block_size: u32,
    chunk_length: u64,
    flash_sector: u32,
    last_rest_size: usize,
    last_rest_data: Vec<u8>,
    rawdata_start_sector: u32,
    rawdata_size: u64,
    checksum: u32,
    verify_enabled: bool,
    total_written: u64,
    chunks_seen: u32,
    logical_blocks: u64,
    logger: &'a Logger,
    written_bytes: Arc<AtomicU64>,
    last_speed_update: Arc<AtomicU64>,
}

impl<'a> SparseParser<'a> {
    /// Create a new sparse parser
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
            chunks_seen: 0,
            logical_blocks: 0,
            logger,
            written_bytes,
            last_speed_update,
        }
    }

    /// Get current checksum
    pub fn checksum(&self) -> u32 {
        self.checksum
    }

    /// Get raw data info for verification
    pub fn rawdata_info(&self) -> (u32, u64) {
        (self.rawdata_start_sector, self.rawdata_size)
    }

    /// Check if verification is needed
    pub fn need_verify(&self) -> bool {
        self.verify_enabled && self.last_chunk_type == LastChunkType::Raw && self.rawdata_size > 0
    }

    /// Get total bytes written
    pub fn total_written(&self) -> u64 {
        self.total_written
    }

    fn finish(&self, expected_chunks: u32, expected_blocks: u32) -> FlashResult<()> {
        if self.last_rest_size != 0 || self.state != ParseState::ChunkHead || self.chunk_length != 0
        {
            return Err(FlashError::InvalidFirmwareFormat(
                "Truncated sparse chunk data".to_string(),
            ));
        }
        if self.chunks_seen != expected_chunks {
            return Err(FlashError::InvalidFirmwareFormat(format!(
                "Sparse chunk count mismatch: expected {}, got {}",
                expected_chunks, self.chunks_seen
            )));
        }
        if self.logical_blocks != u64::from(expected_blocks) {
            return Err(FlashError::InvalidFirmwareFormat(format!(
                "Sparse block count mismatch: expected {}, got {}",
                expected_blocks, self.logical_blocks
            )));
        }
        Ok(())
    }

    /// Parse and download sparse chunks
    ///
    /// Processes buffer data and downloads chunks to device
    pub async fn parse_and_download<C: FesOps>(
        &mut self,
        ctx: &C,
        buffer: &[u8],
        length: usize,
    ) -> FlashResult<()> {
        use crate::firmware::sparse::{ALIGNMENT_SIZE, MIN_DOWNLOAD_SIZE, SECTOR_SIZE};

        if length > buffer.len() {
            return Err(FlashError::InvalidFirmwareFormat(format!(
                "Sparse input length {} exceeds buffer size {}",
                length,
                buffer.len()
            )));
        }

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
                        FlashError::InvalidFirmwareFormat(
                            "Failed to parse chunk header".to_string(),
                        )
                    })?;

                    let chunk_type = chunk.chunk_type;
                    let chunk_sz = chunk.chunk_sz;
                    let total_sz = chunk.total_sz;

                    offset += CHUNK_HEADER_SIZE;
                    this_rest_size -= CHUNK_HEADER_SIZE;

                    self.chunks_seen = self.chunks_seen.checked_add(1).ok_or_else(|| {
                        FlashError::InvalidFirmwareFormat("Sparse chunk count overflow".to_string())
                    })?;
                    self.logical_blocks = self
                        .logical_blocks
                        .checked_add(u64::from(chunk_sz))
                        .ok_or_else(|| {
                            FlashError::InvalidFirmwareFormat(
                                "Sparse logical block count overflow".to_string(),
                            )
                        })?;
                    self.chunk_length = u64::from(chunk_sz) * u64::from(self.block_size);

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
                            if u64::from(total_sz) != self.chunk_length + CHUNK_HEADER_SIZE as u64 {
                                return Err(FlashError::InvalidFirmwareFormat(
                                    "Invalid RAW chunk size".to_string(),
                                ));
                            }

                            if self.last_chunk_type != LastChunkType::Raw {
                                self.checksum = 0;
                                self.rawdata_start_sector = self.flash_sector;
                                self.rawdata_size = 0;
                            }

                            self.logger.debug(&format!(
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

                            self.flash_sector = self
                                .flash_sector
                                .checked_add(
                                    u32::try_from(self.chunk_length / SECTOR_SIZE).map_err(
                                        |_| {
                                            FlashError::InvalidFirmwareFormat(
                                                "DONT_CARE chunk exceeds FES sector range"
                                                    .to_string(),
                                            )
                                        },
                                    )?,
                                )
                                .ok_or_else(|| {
                                    FlashError::InvalidFirmwareFormat(
                                        "DONT_CARE address exceeds FES sector range".to_string(),
                                    )
                                })?;
                            self.chunk_length = 0;
                            self.state = ParseState::ChunkHead;
                            self.last_chunk_type = LastChunkType::DontCare;
                        }

                        CHUNK_TYPE_CRC32 => {
                            if total_sz != CHUNK_HEADER_SIZE as u32 + 4 || chunk_sz != 0 {
                                return Err(FlashError::InvalidFirmwareFormat(
                                    "Invalid CRC32 chunk size".to_string(),
                                ));
                            }
                            self.state = ParseState::ChunkCrcData;
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
                    let unenough_length = self.chunk_length.saturating_sub(this_rest_size as u64);

                    if unenough_length == 0 {
                        let chunk_length = usize::try_from(self.chunk_length).map_err(|_| {
                            FlashError::InvalidFirmwareFormat(
                                "Sparse RAW chunk exceeds host address space".to_string(),
                            )
                        })?;
                        let data = &work_buffer[offset..offset + chunk_length];
                        self.download_data(ctx, data, true)?;

                        this_rest_size -= chunk_length;
                        offset += chunk_length;
                        self.chunk_length = 0;
                        self.state = ParseState::ChunkHead;
                    } else {
                        if this_rest_size < MIN_DOWNLOAD_SIZE {
                            self.save_rest_data(work_buffer, offset, this_rest_size);
                            return Ok(());
                        }

                        let download_size = if unenough_length < ALIGNMENT_SIZE as u64 {
                            this_rest_size + unenough_length as usize - ALIGNMENT_SIZE
                        } else {
                            this_rest_size & !(ALIGNMENT_SIZE - 1)
                        };

                        let data = &work_buffer[offset..offset + download_size];
                        self.download_data(ctx, data, true)?;

                        offset += download_size;
                        self.chunk_length -= download_size as u64;
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

                    self.logger.debug(&format!(
                        "Downloading FILL chunk at sector 0x{:x}, size {} bytes, fill value 0x{:x}",
                        self.flash_sector, self.chunk_length, fill_value
                    ));

                    self.process_fill_chunk(ctx, fill_value)?;
                    self.chunk_length = 0;
                    self.state = ParseState::ChunkHead;
                }

                ParseState::ChunkCrcData => {
                    if this_rest_size < 4 {
                        self.save_rest_data(work_buffer, offset, this_rest_size);
                        return Ok(());
                    }
                    offset += 4;
                    this_rest_size -= 4;
                    self.state = ParseState::ChunkHead;
                }
            }
        }

        Ok(())
    }

    /// Save remaining data for next iteration
    fn save_rest_data(&mut self, buffer: &[u8], offset: usize, rest_size: usize) {
        self.last_rest_size = rest_size;
        if rest_size > 0 {
            self.last_rest_data = buffer[offset..offset + rest_size].to_vec();
        }
    }

    /// Download data to device
    fn download_data<C: FesOps>(
        &mut self,
        ctx: &C,
        data: &[u8],
        update_verify: bool,
    ) -> FlashResult<()> {
        use crate::firmware::sparse::{add_sum, SECTOR_SIZE};

        if data.is_empty() {
            return Ok(());
        }

        let sector = self.flash_sector;
        let written_bytes = Arc::clone(&self.written_bytes);
        let last_speed_update = Arc::clone(&self.last_speed_update);
        let logger = self.logger;
        let chunk_base_bytes = self.written_bytes.load(Ordering::SeqCst);
        let data_length = u64::try_from(data.len()).map_err(|_| {
            FlashError::InvalidFirmwareFormat("Sparse write exceeds host address space".to_string())
        })?;
        let sector_count = u32::try_from(data_length / SECTOR_SIZE).map_err(|_| {
            FlashError::InvalidFirmwareFormat("Sparse write exceeds FES sector range".to_string())
        })?;
        let next_sector = self.flash_sector.checked_add(sector_count).ok_or_else(|| {
            FlashError::InvalidFirmwareFormat(
                "Sparse write address exceeds FES sector range".to_string(),
            )
        })?;

        let written = ctx
            .fes_down_with_progress(data, sector, FesDataType::Flash, move |written, _total| {
                let current = chunk_base_bytes + written;
                written_bytes.store(current, Ordering::SeqCst);
                let last = last_speed_update.load(Ordering::SeqCst);

                if current.saturating_sub(last) >= constants::SPEED_UPDATE_INTERVAL {
                    last_speed_update.store(current, Ordering::SeqCst);
                    logger.update_progress_with_speed(current);
                }
            })
            .map_err(|e| FlashError::UsbTransferError(e.to_string()))?;

        if written != data.len() as u64 {
            return Err(FlashError::PartitionDownloadFailed(format!(
                "Short sparse write: expected {} bytes, wrote {}",
                data.len(),
                written
            )));
        }

        if update_verify {
            self.checksum = add_sum(data, self.checksum);
            self.rawdata_size += written;
        }
        self.total_written += written;
        self.flash_sector = next_sector;

        Ok(())
    }

    /// Process fill chunk (write repeated pattern)
    fn process_fill_chunk<C: FesOps>(&mut self, ctx: &C, fill_value: u32) -> FlashResult<()> {
        use crate::firmware::sparse::{MAX_FILL_COUNT, SECTOR_SIZE};

        if self.chunk_length == 0 {
            return Ok(());
        }

        if !self.chunk_length.is_multiple_of(SECTOR_SIZE) {
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
        for chunk in fill_buffer.as_chunks_mut::<4>().0 {
            chunk.copy_from_slice(&fill_value.to_le_bytes());
        }

        let mut remaining = self.chunk_length;

        while remaining >= u64::from(MAX_FILL_COUNT * 16) {
            self.download_data(ctx, &fill_buffer, false)?;
            remaining -= u64::from(MAX_FILL_COUNT * 16);
        }

        if remaining > 0 {
            let remaining_usize = usize::try_from(remaining).map_err(|_| {
                FlashError::InvalidFirmwareFormat(
                    "Sparse fill chunk exceeds host address space".to_string(),
                )
            })?;
            self.download_data(ctx, &fill_buffer[..remaining_usize], false)?;
        }

        Ok(())
    }

    /// Verify last RAW chunk
    async fn verify_last_chunk<C: FesOps>(&mut self, ctx: &C) -> FlashResult<()> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::firmware::sparse::{
        add_sum, SparseHeader, CHUNK_TYPE_CRC32, SPARSE_HEADER_MAGIC, SPARSE_HEADER_MAJOR_VER,
    };
    use crate::flash::fes_handler::types::PartitionSource;
    use crate::flash::protocol::{tests::MockProtocol, VerifyResponse};
    use crate::test_support::{test_firmware, write_struct, FirmwareEntry};
    use std::io::Cursor;

    fn logger() -> Logger {
        Logger::for_events(false, crate::flash::FlashEventSink::none())
    }

    fn sparse_header(blocks: u32, chunks: u32) -> SparseHeader {
        SparseHeader {
            magic: SPARSE_HEADER_MAGIC,
            major_version: SPARSE_HEADER_MAJOR_VER,
            minor_version: 0,
            file_hdr_sz: SPARSE_HEADER_SIZE as u16,
            chunk_hdr_sz: CHUNK_HEADER_SIZE as u16,
            blk_sz: 512,
            total_blks: blocks,
            total_chunks: chunks,
            image_checksum: 0,
        }
    }

    fn append_struct<T: Copy>(bytes: &mut Vec<u8>, value: &T) {
        let start = bytes.len();
        bytes.resize(start + std::mem::size_of::<T>(), 0);
        write_struct(&mut bytes[start..], value);
    }

    fn raw_sparse(payload: &[u8; 512]) -> Vec<u8> {
        let mut bytes = Vec::new();
        append_struct(&mut bytes, &sparse_header(1, 1));
        append_struct(
            &mut bytes,
            &ChunkHeader {
                chunk_type: CHUNK_TYPE_RAW,
                reserved: 0,
                chunk_sz: 1,
                total_sz: CHUNK_HEADER_SIZE as u32 + 512,
            },
        );
        bytes.extend_from_slice(payload);
        bytes
    }

    fn mixed_sparse(payload: &[u8; 512]) -> Vec<u8> {
        let mut bytes = Vec::new();
        append_struct(&mut bytes, &sparse_header(3, 4));
        append_struct(
            &mut bytes,
            &ChunkHeader {
                chunk_type: CHUNK_TYPE_RAW,
                reserved: 0,
                chunk_sz: 1,
                total_sz: CHUNK_HEADER_SIZE as u32 + 512,
            },
        );
        bytes.extend_from_slice(payload);
        append_struct(
            &mut bytes,
            &ChunkHeader {
                chunk_type: CHUNK_TYPE_FILL,
                reserved: 0,
                chunk_sz: 1,
                total_sz: CHUNK_HEADER_SIZE as u32 + 4,
            },
        );
        bytes.extend_from_slice(&0xaabb_ccddu32.to_le_bytes());
        append_struct(
            &mut bytes,
            &ChunkHeader {
                chunk_type: CHUNK_TYPE_DONT_CARE,
                reserved: 0,
                chunk_sz: 1,
                total_sz: CHUNK_HEADER_SIZE as u32,
            },
        );
        append_struct(
            &mut bytes,
            &ChunkHeader {
                chunk_type: CHUNK_TYPE_CRC32,
                reserved: 0,
                chunk_sz: 0,
                total_sz: CHUNK_HEADER_SIZE as u32 + 4,
            },
        );
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes
    }

    fn info(length: usize) -> PartitionDownloadInfo {
        PartitionDownloadInfo {
            partition_name: "system".to_string(),
            partition_address: 0x20,
            download_filename: "system.img".to_string(),
            download_subtype: "SYSTEM0000000000".to_string(),
            data_offset: 0,
            data_length: length as u64,
            source: PartitionSource::Firmware,
            wrap_address: false,
        }
    }

    fn new_parser(logger: &Logger, verify: bool) -> SparseParser<'_> {
        SparseParser::new(
            512,
            0x20,
            verify,
            logger,
            Arc::new(AtomicU64::new(0)),
            Arc::new(AtomicU64::new(0)),
        )
    }

    #[tokio::test]
    async fn execute_streams_primary_and_fallback_entries_and_verifies_raw_data() {
        let payload = [0x5au8; 512];
        let sparse = raw_sparse(&payload);
        for maintype in [super::super::super::types::ITEM_ROOTFSFAT16, "12345678"] {
            let firmware = test_firmware(&[FirmwareEntry {
                filename: "system.img",
                maintype,
                subtype: "SYSTEM0000000000",
                data: &sparse,
            }]);
            let mut packer = OpenixPacker::new();
            packer.load(firmware.path()).unwrap();
            let logger = logger();
            let ctx = MockProtocol::default();
            ctx.verify_values
                .borrow_mut()
                .push_back(Ok(
                    MockProtocol::valid_response(add_sum(&payload, 0) as i32),
                ));
            SparseDownloader::new(
                &logger,
                Arc::new(AtomicU64::new(0)),
                Arc::new(AtomicU64::new(0)),
            )
            .execute(&ctx, &mut packer, &info(sparse.len()), true)
            .await
            .unwrap();
            let downloads = ctx.downloads.borrow();
            assert_eq!(downloads.len(), 1);
            assert_eq!(downloads[0].addr, 0x20);
            assert_eq!(downloads[0].data, payload);
            assert_eq!(&*ctx.verify_value_calls.borrow(), &[(0x20, 512)]);
        }

        let empty = test_firmware(&[]);
        let mut packer = OpenixPacker::new();
        packer.load(empty.path()).unwrap();
        let logger = logger();
        let ctx = MockProtocol::default();
        let mut invalid = info(1);
        invalid.partition_address = u64::from(u32::MAX) + 1;
        assert!(matches!(
            SparseDownloader::new(
                &logger,
                Arc::new(AtomicU64::new(0)),
                Arc::new(AtomicU64::new(0)),
            )
            .execute(&ctx, &mut packer, &invalid, false)
            .await,
            Err(FlashError::InvalidFirmwareFormat(_))
        ));
        assert!(ctx.downloads.borrow().is_empty());
    }

    #[tokio::test]
    async fn mixed_chunks_verify_transition_write_fill_skip_holes_and_consume_crc() {
        let payload = [1u8; 512];
        let sparse = mixed_sparse(&payload);
        let firmware = test_firmware(&[FirmwareEntry {
            filename: "system.img",
            maintype: "RFSFAT16",
            subtype: "SYSTEM0000000000",
            data: &sparse,
        }]);
        let mut packer = OpenixPacker::new();
        packer.load(firmware.path()).unwrap();
        let logger = logger();
        let ctx = MockProtocol::default();
        ctx.verify_values
            .borrow_mut()
            .push_back(Ok(
                MockProtocol::valid_response(add_sum(&payload, 0) as i32),
            ));
        SparseDownloader::new(
            &logger,
            Arc::new(AtomicU64::new(0)),
            Arc::new(AtomicU64::new(0)),
        )
        .execute(&ctx, &mut packer, &info(sparse.len()), true)
        .await
        .unwrap();

        let downloads = ctx.downloads.borrow();
        assert_eq!(downloads.len(), 2);
        assert_eq!(downloads[0].addr, 0x20);
        assert_eq!(downloads[1].addr, 0x21);
        assert_eq!(&downloads[1].data[..4], &0xaabb_ccddu32.to_le_bytes());
        assert_eq!(ctx.verify_value_calls.borrow().len(), 1);
    }

    #[tokio::test]
    async fn truncated_and_count_mismatched_sparse_images_are_rejected() {
        let mut cases = Vec::new();
        let mut missing_chunk = Vec::new();
        append_struct(&mut missing_chunk, &sparse_header(1, 1));
        cases.push(missing_chunk);

        let mut truncated_raw = Vec::new();
        append_struct(&mut truncated_raw, &sparse_header(1, 1));
        append_struct(
            &mut truncated_raw,
            &ChunkHeader {
                chunk_type: CHUNK_TYPE_RAW,
                reserved: 0,
                chunk_sz: 1,
                total_sz: CHUNK_HEADER_SIZE as u32 + 512,
            },
        );
        truncated_raw.extend_from_slice(&[0; 100]);
        cases.push(truncated_raw);

        let payload = [0u8; 512];
        let mut wrong_blocks = raw_sparse(&payload);
        let header = sparse_header(2, 1);
        write_struct(&mut wrong_blocks[..SPARSE_HEADER_SIZE], &header);
        cases.push(wrong_blocks);

        for data in cases {
            let firmware = test_firmware(&[FirmwareEntry {
                filename: "system.img",
                maintype: "RFSFAT16",
                subtype: "SYSTEM0000000000",
                data: &data,
            }]);
            let mut packer = OpenixPacker::new();
            packer.load(firmware.path()).unwrap();
            let logger = logger();
            let ctx = MockProtocol::default();
            assert!(matches!(
                SparseDownloader::new(
                    &logger,
                    Arc::new(AtomicU64::new(0)),
                    Arc::new(AtomicU64::new(0))
                )
                .execute(&ctx, &mut packer, &info(data.len()), false)
                .await,
                Err(FlashError::InvalidFirmwareFormat(_))
            ));
        }
    }

    #[tokio::test]
    async fn parser_rejects_invalid_lengths_chunk_headers_and_short_writes() {
        let logger = logger();
        let ctx = MockProtocol::default();
        let mut parser = new_parser(&logger, false);
        assert!(matches!(
            parser.parse_and_download(&ctx, &[0], 2).await,
            Err(FlashError::InvalidFirmwareFormat(_))
        ));

        for chunk in [
            ChunkHeader {
                chunk_type: 0xffff,
                reserved: 0,
                chunk_sz: 0,
                total_sz: 12,
            },
            ChunkHeader {
                chunk_type: CHUNK_TYPE_RAW,
                reserved: 0,
                chunk_sz: 1,
                total_sz: 12,
            },
            ChunkHeader {
                chunk_type: CHUNK_TYPE_FILL,
                reserved: 0,
                chunk_sz: 1,
                total_sz: 12,
            },
            ChunkHeader {
                chunk_type: CHUNK_TYPE_DONT_CARE,
                reserved: 0,
                chunk_sz: 1,
                total_sz: 16,
            },
            ChunkHeader {
                chunk_type: CHUNK_TYPE_CRC32,
                reserved: 0,
                chunk_sz: 1,
                total_sz: 16,
            },
        ] {
            let mut input = vec![0; SPARSE_HEADER_SIZE];
            append_struct(&mut input, &chunk);
            let mut parser = new_parser(&logger, false);
            assert!(matches!(
                parser.parse_and_download(&ctx, &input, input.len()).await,
                Err(FlashError::InvalidFirmwareFormat(_))
            ));
        }

        let mut direct = new_parser(&logger, false);
        assert!(direct.download_data(&ctx, &[], true).is_ok());
        ctx.progress_written_override.set(Some(1));
        assert!(matches!(
            direct.download_data(&ctx, &[0; 512], true),
            Err(FlashError::PartitionDownloadFailed(_))
        ));
        ctx.progress_written_override.set(None);
        *ctx.fail_down.borrow_mut() = Some("usb".to_string());
        assert!(matches!(
            direct.download_data(&ctx, &[0; 512], true),
            Err(FlashError::UsbTransferError(_))
        ));
    }

    #[tokio::test]
    async fn parser_handles_fragmented_input_and_verification_failures() {
        let payload = [7u8; 512];
        let data = raw_sparse(&payload);
        let logger = logger();
        let ctx = MockProtocol::default();
        let mut parser = new_parser(&logger, true);
        parser
            .parse_and_download(&ctx, &data[..10], 10)
            .await
            .unwrap();
        parser
            .parse_and_download(&ctx, &data[10..], data.len() - 10)
            .await
            .unwrap();
        parser.finish(1, 1).unwrap();
        assert!(parser.need_verify());
        assert_eq!(parser.rawdata_info(), (0x20, 512));
        assert_eq!(parser.total_written(), 512);
        assert_eq!(parser.checksum(), add_sum(&payload, 0));

        ctx.verify_values
            .borrow_mut()
            .push_back(Ok(MockProtocol::valid_response(parser.checksum() as i32)));
        parser.verify_last_chunk(&ctx).await.unwrap();
        assert!(!parser.need_verify());

        for response in [
            Ok(MockProtocol::valid_response(1)),
            Ok(VerifyResponse {
                flag: 0,
                media_crc: 0,
            }),
            Err("verify".to_string()),
        ] {
            let mut parser = new_parser(&logger, true);
            parser.last_chunk_type = LastChunkType::Raw;
            parser.rawdata_size = 512;
            parser.checksum = 2;
            let ctx = MockProtocol::default();
            ctx.verify_values.borrow_mut().push_back(response);
            assert!(parser.verify_last_chunk(&ctx).await.is_err());
        }

        let mut empty = new_parser(&logger, true);
        empty
            .verify_last_chunk(&MockProtocol::default())
            .await
            .unwrap();
    }

    #[test]
    fn fill_processing_checks_alignment_zero_short_write_and_address_overflow() {
        let logger = logger();
        let ctx = MockProtocol::default();
        let mut parser = new_parser(&logger, false);
        parser.process_fill_chunk(&ctx, 1).unwrap();
        parser.chunk_length = 1;
        assert!(matches!(
            parser.process_fill_chunk(&ctx, 1),
            Err(FlashError::InvalidFirmwareFormat(_))
        ));
        parser.chunk_length = 512;
        parser.flash_sector = u32::MAX;
        assert!(matches!(
            parser.process_fill_chunk(&ctx, 1),
            Err(FlashError::InvalidFirmwareFormat(_))
        ));
        assert!(ctx.downloads.borrow().is_empty());
    }

    #[test]
    fn packer_entry_reader_supports_read_seek_fallback_and_errors() {
        let firmware = test_firmware(&[FirmwareEntry {
            filename: "system.img",
            maintype: "12345678",
            subtype: "SYSTEM0000000000",
            data: b"abcdef",
        }]);
        let mut packer = OpenixPacker::new();
        packer.load(firmware.path()).unwrap();
        let mut reader = PackerEntryReader::new(&mut packer, "SYSTEM0000000000", 6);
        let mut buffer = [0; 2];
        assert_eq!(reader.read(&mut buffer).unwrap(), 2);
        assert_eq!(&buffer, b"ab");
        assert_eq!(reader.seek(SeekFrom::Current(1)).unwrap(), 3);
        assert_eq!(reader.seek(SeekFrom::End(-1)).unwrap(), 5);
        assert_eq!(reader.read(&mut buffer).unwrap(), 1);
        assert_eq!(reader.read(&mut []).unwrap(), 0);
        assert!(reader.seek(SeekFrom::Current(-100)).is_err());

        let empty_firmware = test_firmware(&[]);
        let mut empty_packer = OpenixPacker::new();
        empty_packer.load(empty_firmware.path()).unwrap();
        let mut missing = PackerEntryReader::new(&mut empty_packer, "MISSING000000000", 1);
        assert!(missing.read(&mut buffer).is_err());
    }

    #[tokio::test]
    async fn reader_path_reports_short_header_and_bad_seek() {
        struct BadSeek;
        impl Read for BadSeek {
            fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
                Ok(0)
            }
        }
        impl Seek for BadSeek {
            fn seek(&mut self, _pos: SeekFrom) -> io::Result<u64> {
                Err(io::Error::other("seek"))
            }
        }

        let logger = logger();
        let handler = SparseDownloader::new(
            &logger,
            Arc::new(AtomicU64::new(0)),
            Arc::new(AtomicU64::new(0)),
        );
        let ctx = MockProtocol::default();
        let params = SparseDownloadParams {
            data_offset: 0,
            data_length: 0,
            start_sector: 0,
            partition_name: "system",
            verify_enabled: false,
        };
        assert!(matches!(
            handler
                .download_sparse_from_reader(&ctx, &mut BadSeek, &params)
                .await,
            Err(FlashError::InvalidFirmwareFormat(_))
        ));
        assert!(matches!(
            handler
                .download_sparse_from_reader(&ctx, &mut Cursor::new(Vec::<u8>::new()), &params)
                .await,
            Err(FlashError::InvalidFirmwareFormat(_))
        ));
    }
}
