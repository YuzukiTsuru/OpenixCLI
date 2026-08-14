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
            && self.blk_sz != 0
            && self.blk_sz.is_multiple_of(SECTOR_SIZE as u32)
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

    let block_size = header.blk_sz;
    if block_size == 0 || !block_size.is_multiple_of(SECTOR_SIZE as u32) {
        return Err(FlashError::InvalidFirmwareFormat(format!(
            "Invalid sparse block size: {}",
            block_size
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

    let mut tail = [0u8; 4];
    tail[..data.len() - aligned_len].copy_from_slice(&data[aligned_len..]);
    sum = sum.wrapping_add(u32::from_le_bytes(tail));

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
    ChunkCrcData,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::write_struct;
    use crate::utils::FlashError;

    fn valid_header() -> SparseHeader {
        SparseHeader {
            magic: SPARSE_HEADER_MAGIC,
            major_version: SPARSE_HEADER_MAJOR_VER,
            minor_version: 0,
            file_hdr_sz: SPARSE_HEADER_SIZE as u16,
            chunk_hdr_sz: CHUNK_HEADER_SIZE as u16,
            blk_sz: 4096,
            total_blks: 2,
            total_chunks: 1,
            image_checksum: 0,
        }
    }

    fn header_bytes(header: SparseHeader) -> Vec<u8> {
        let mut bytes = vec![0; SPARSE_HEADER_SIZE];
        write_struct(&mut bytes, &header);
        bytes
    }

    fn error_message(error: FlashError) -> String {
        match error {
            FlashError::InvalidFirmwareFormat(message) => message,
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn sparse_header_parsing_and_mutation_cover_boundaries() {
        assert!(SparseHeader::parse(&[]).is_none());
        assert!(SparseHeader::parse_mut(&mut []).is_none());
        assert!(!is_sparse_format(&[]));

        let mut bytes = header_bytes(valid_header());
        assert!(SparseHeader::parse(&bytes).unwrap().is_valid());
        assert!(is_sparse_format(&bytes));
        SparseHeader::parse_mut(&mut bytes).unwrap().magic = 0;
        assert!(!SparseHeader::parse(&bytes).unwrap().is_valid());
        assert!(!is_sparse_format(&bytes));
    }

    #[test]
    fn sparse_probe_reports_each_invalid_header_field() {
        assert!(error_message(sparse_format_probe(&[]).unwrap_err()).contains("insufficient"));

        let mut header = valid_header();
        header.magic = 0;
        assert!(
            error_message(sparse_format_probe(&header_bytes(header)).unwrap_err())
                .contains("magic")
        );
        header = valid_header();
        header.major_version = 2;
        assert!(
            error_message(sparse_format_probe(&header_bytes(header)).unwrap_err())
                .contains("version")
        );
        header = valid_header();
        header.file_hdr_sz = 0;
        assert!(
            error_message(sparse_format_probe(&header_bytes(header)).unwrap_err())
                .contains("file header")
        );
        header = valid_header();
        header.chunk_hdr_sz = 0;
        assert!(
            error_message(sparse_format_probe(&header_bytes(header)).unwrap_err())
                .contains("chunk header")
        );
        header = valid_header();
        header.blk_sz = 0;
        assert!(
            error_message(sparse_format_probe(&header_bytes(header)).unwrap_err())
                .contains("block size")
        );
        header = valid_header();
        header.blk_sz = 513;
        assert!(!is_sparse_format(&header_bytes(header)));

        let parsed = sparse_format_probe(&header_bytes(valid_header())).unwrap();
        let block_size = parsed.blk_sz;
        assert_eq!(block_size, 4096);
    }

    #[test]
    fn chunk_header_parsing_mutation_and_saturating_size() {
        assert!(ChunkHeader::parse(&[]).is_none());
        assert!(ChunkHeader::parse_mut(&mut []).is_none());

        let chunk = ChunkHeader {
            chunk_type: CHUNK_TYPE_RAW,
            reserved: 0,
            chunk_sz: 1,
            total_sz: CHUNK_HEADER_SIZE as u32 + 4,
        };
        let mut bytes = vec![0; CHUNK_HEADER_SIZE];
        write_struct(&mut bytes, &chunk);
        assert_eq!(ChunkHeader::parse(&bytes).unwrap().data_size(), 4);
        ChunkHeader::parse_mut(&mut bytes).unwrap().total_sz = 1;
        assert_eq!(ChunkHeader::parse(&bytes).unwrap().data_size(), 0);
    }

    #[test]
    fn additive_checksum_handles_all_tail_lengths_and_wraps() {
        assert_eq!(add_sum(&[], 7), 7);
        assert_eq!(add_sum(&[1], 0), 1);
        assert_eq!(add_sum(&[1, 2], 0), 0x0201);
        assert_eq!(add_sum(&[1, 2, 3], 0), 0x030201);
        assert_eq!(add_sum(&[1, 2, 3, 4], 0), 0x04030201);
        assert_eq!(add_sum(&[1, 0, 0, 0], u32::MAX), 0);
        assert_eq!(add_sum(&[1, 0, 0, 0, 2], 0), 3);
    }
}
