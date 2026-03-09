#![allow(dead_code)]

pub const SPARSE_HEADER_MAGIC: u32 = 0xed26ff3a;
pub const SPARSE_HEADER_MAJOR_VER: u16 = 1;

pub const CHUNK_TYPE_RAW: u16 = 0xcac1;
pub const CHUNK_TYPE_FILL: u16 = 0xcac2;
pub const CHUNK_TYPE_DONT_CARE: u16 = 0xcac3;
pub const CHUNK_TYPE_CRC32: u16 = 0xcac4;

pub const SPARSE_HEADER_SIZE: usize = 28;
pub const CHUNK_HEADER_SIZE: usize = 12;

pub const SECTOR_SIZE: u64 = 512;
pub const MIN_DOWNLOAD_SIZE: usize = 8 * 1024;
pub const ALIGNMENT_SIZE: usize = 4 * 1024;
pub const MAX_FILL_COUNT: u32 = 4096;

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct SparseHeader {
    pub magic: u32,
    pub major_version: u16,
    pub minor_version: u16,
    pub file_hdr_sz: u16,
    pub chunk_hdr_sz: u16,
    pub blk_sz: u32,
    pub total_blks: u32,
    pub total_chunks: u32,
    pub image_checksum: u32,
}

impl SparseHeader {
    pub fn parse(data: &[u8]) -> Option<&Self> {
        if data.len() < SPARSE_HEADER_SIZE {
            return None;
        }
        let ptr = data.as_ptr() as *const SparseHeader;
        Some(unsafe { &*ptr })
    }

    pub fn parse_mut(data: &mut [u8]) -> Option<&mut Self> {
        if data.len() < SPARSE_HEADER_SIZE {
            return None;
        }
        let ptr = data.as_mut_ptr() as *mut SparseHeader;
        Some(unsafe { &mut *ptr })
    }

    pub fn is_valid(&self) -> bool {
        self.magic == SPARSE_HEADER_MAGIC
            && self.major_version == SPARSE_HEADER_MAJOR_VER
            && self.file_hdr_sz as usize == SPARSE_HEADER_SIZE
            && self.chunk_hdr_sz as usize == CHUNK_HEADER_SIZE
    }
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct ChunkHeader {
    pub chunk_type: u16,
    pub reserved: u16,
    pub chunk_sz: u32,
    pub total_sz: u32,
}

impl ChunkHeader {
    pub fn parse(data: &[u8]) -> Option<&Self> {
        if data.len() < CHUNK_HEADER_SIZE {
            return None;
        }
        let ptr = data.as_ptr() as *const ChunkHeader;
        Some(unsafe { &*ptr })
    }

    pub fn parse_mut(data: &mut [u8]) -> Option<&mut Self> {
        if data.len() < CHUNK_HEADER_SIZE {
            return None;
        }
        let ptr = data.as_mut_ptr() as *mut ChunkHeader;
        Some(unsafe { &mut *ptr })
    }

    pub fn data_size(&self) -> u32 {
        self.total_sz.saturating_sub(CHUNK_HEADER_SIZE as u32)
    }
}

pub fn is_sparse_format(data: &[u8]) -> bool {
    if let Some(header) = SparseHeader::parse(data) {
        header.is_valid()
    } else {
        false
    }
}

pub fn sparse_format_probe(data: &[u8]) -> crate::utils::FlashResult<SparseHeader> {
    use crate::utils::FlashError;
    
    let header = SparseHeader::parse(data).ok_or_else(|| {
        FlashError::InvalidFirmwareFormat("Failed to parse sparse header: insufficient data".to_string())
    })?;

    let magic = header.magic;
    let major_version = header.major_version;
    let file_hdr_sz = header.file_hdr_sz;
    let chunk_hdr_sz = header.chunk_hdr_sz;

    if magic != SPARSE_HEADER_MAGIC {
        return Err(FlashError::InvalidFirmwareFormat(format!(
            "Invalid sparse magic: expected 0x{:08x}, got 0x{:08x}",
            SPARSE_HEADER_MAGIC, magic
        )));
    }

    if major_version != SPARSE_HEADER_MAJOR_VER {
        return Err(FlashError::InvalidFirmwareFormat(format!(
            "Unsupported sparse version: {}",
            major_version
        )));
    }

    if file_hdr_sz as usize != SPARSE_HEADER_SIZE {
        return Err(FlashError::InvalidFirmwareFormat(format!(
            "Invalid file header size: expected {}, got {}",
            SPARSE_HEADER_SIZE, file_hdr_sz
        )));
    }

    if chunk_hdr_sz as usize != CHUNK_HEADER_SIZE {
        return Err(FlashError::InvalidFirmwareFormat(format!(
            "Invalid chunk header size: expected {}, got {}",
            CHUNK_HEADER_SIZE, chunk_hdr_sz
        )));
    }

    Ok(*header)
}

pub fn add_sum(data: &[u8], initial: u32) -> u32 {
    let mut sum = initial;
    let aligned_len = data.len() & !0x03;

    for i in (0..aligned_len).step_by(4) {
        let value = u32::from_le_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]);
        sum = sum.wrapping_add(value);
    }

    let remaining = data.len() & 0x03;
    if remaining > 0 {
        let last_value: u32 = match remaining {
            1 => data[aligned_len] as u32,
            2 => data[aligned_len] as u32 | (data[aligned_len + 1] as u32) << 8,
            3 => {
                data[aligned_len] as u32
                    | (data[aligned_len + 1] as u32) << 8
                    | (data[aligned_len + 2] as u32) << 16
            }
            _ => 0,
        };
        sum = sum.wrapping_add(last_value);
    }

    sum
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LastChunkType {
    Undefine,
    Raw,
    Fill,
    DontCare,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ParseState {
    TotalHead,
    ChunkHead,
    ChunkData,
    ChunkFillData,
}

pub struct SparseParser<'a> {
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
    logger: &'a crate::utils::Logger,
}

impl<'a> SparseParser<'a> {
    pub fn new(
        block_size: u32,
        start_sector: u32,
        verify_enabled: bool,
        logger: &'a crate::utils::Logger,
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
        }
    }

    pub fn flash_sector(&self) -> u32 {
        self.flash_sector
    }

    pub fn checksum(&self) -> u32 {
        self.checksum
    }

    pub fn rawdata_info(&self) -> (u32, u64) {
        (self.rawdata_start_sector, self.rawdata_size)
    }

    pub fn last_chunk_type(&self) -> LastChunkType {
        self.last_chunk_type
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
    ) -> crate::utils::FlashResult<()> {
        use crate::utils::FlashError;

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
        partition_name: &str,
        _partition_total_bytes: u64,
    ) -> crate::utils::FlashResult<()> {
        use crate::utils::FlashError;
        use libefex::FesDataType;

        if data.is_empty() {
            return Ok(());
        }

        let sector = self.flash_sector;
        let partition_name_str = partition_name.to_string();
        let total_written = self.total_written;
        let logger = self.logger;

        let written = ctx
            .fes_down_with_progress(data, sector, FesDataType::Flash, move |written, _total| {
                let partition_written = total_written + written;
                logger.progress_update(
                    partition_written as usize,
                    &partition_name_str,
                );
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
    ) -> crate::utils::FlashResult<()> {
        use crate::utils::FlashError;

        if self.chunk_length == 0 {
            return Ok(());
        }

        if self.chunk_length % SECTOR_SIZE as u32 != 0 {
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

    async fn verify_last_chunk(&mut self, ctx: &libefex::Context) -> crate::utils::FlashResult<()> {
        use crate::config::mbr_parser::EFEX_CRC32_VALID_FLAG;
        use crate::utils::FlashError;

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
