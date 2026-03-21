//! FES handler type definitions
//!
//! Provides types and structures used by FES handler

/// Item type for root filesystem FAT16 partition
pub const ITEM_ROOTFSFAT16: &str = "RFSFAT16";

/// FES data type values for verify operations
pub mod fes_data_type {
    pub const DRAM: u32 = 0x7f00;
    pub const MBR: u32 = 0x7f01;
    pub const BOOT1: u32 = 0x7f02;
    pub const BOOT0: u32 = 0x7f03;
    pub const ERASE: u32 = 0x7f04;
    pub const FULL_IMG_SIZE: u32 = 0x7f10;
    pub const EXT4_UBIFS: u32 = 0x7ff0;
    pub const FLASH: u32 = 0x8000;
}

/// Information about a partition to be downloaded
///
/// # Fields
/// * `partition_name` - Name of the partition
/// * `partition_address` - Starting address on storage
/// * `download_filename` - Filename in firmware
/// * `download_subtype` - Subtype for firmware lookup
/// * `data_offset` - Offset in firmware file
/// * `data_length` - Size of partition data
#[derive(Debug, Clone)]
pub struct PartitionDownloadInfo {
    pub partition_name: String,
    pub partition_address: u64,
    pub download_filename: String,
    pub download_subtype: String,
    pub data_offset: u64,
    pub data_length: u64,
}

/// Incremental checksum calculator
///
/// Calculates 32-bit incremental checksum for data verification
pub struct IncrementalChecksum {
    sum: u32,
    pending_bytes: Vec<u8>,
}

impl IncrementalChecksum {
    /// Create a new checksum calculator
    pub fn new() -> Self {
        IncrementalChecksum {
            sum: 0,
            pending_bytes: Vec::new(),
        }
    }

    /// Update checksum with more data
    ///
    /// Processes data in 4-byte aligned chunks
    pub fn update(&mut self, data: &[u8]) {
        let buffer = if !self.pending_bytes.is_empty() {
            let mut combined = self.pending_bytes.clone();
            combined.extend_from_slice(data);
            self.pending_bytes.clear();
            combined
        } else {
            data.to_vec()
        };

        let aligned_length = buffer.len() & !0x03;
        let remaining = buffer.len() & 0x03;

        for i in (0..aligned_length).step_by(4) {
            let value =
                u32::from_le_bytes([buffer[i], buffer[i + 1], buffer[i + 2], buffer[i + 3]]);
            self.sum = self.sum.wrapping_add(value);
        }

        if remaining > 0 {
            self.pending_bytes = buffer[aligned_length..].to_vec();
        }
    }

    /// Finalize and return the checksum
    ///
    /// Processes any remaining bytes and returns the final checksum
    pub fn finalize(&mut self) -> u32 {
        if !self.pending_bytes.is_empty() {
            let last_value: u32 = match self.pending_bytes.len() {
                1 => self.pending_bytes[0] as u32 & 0x000000ff,
                2 => {
                    (self.pending_bytes[0] as u32 | (self.pending_bytes[1] as u32) << 8)
                        & 0x0000ffff
                }
                3 => {
                    (self.pending_bytes[0] as u32
                        | (self.pending_bytes[1] as u32) << 8
                        | (self.pending_bytes[2] as u32) << 16)
                        & 0x00ffffff
                }
                _ => 0,
            };
            self.sum = self.sum.wrapping_add(last_value);
            self.pending_bytes.clear();
        }
        self.sum
    }
}

impl Default for IncrementalChecksum {
    fn default() -> Self {
        Self::new()
    }
}
