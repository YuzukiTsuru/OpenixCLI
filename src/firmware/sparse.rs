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
        FlashError::InvalidFirmwareFormat(
            "Failed to parse sparse header: insufficient data".to_string(),
        )
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
