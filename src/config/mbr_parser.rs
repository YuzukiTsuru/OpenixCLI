//! MBR (Master Boot Record) parser
//!
//! Provides parsers for Allwinner MBR partition table format

#![allow(dead_code)]

/// MBR magic string
pub const MBR_MAGIC: &str = "softw411";
/// MBR version
pub const MBR_VERSION: u32 = 0x00000200;
/// MBR size in bytes (16KB)
pub const MBR_SIZE: usize = 16 * 1024;
/// Maximum partition name length
pub const PART_NAME_MAX_LEN: usize = 16;
/// Reserved size in partition entry
pub const PART_SIZE_RES_LEN: usize = 68;
/// Maximum number of partitions supported
pub const MBR_MAX_PART_CNT: usize = 120;

/// CRC32 valid flag
pub const EFEX_CRC32_VALID_FLAG: u32 = 0x6a617603;

/// Raw partition entry structure
///
/// Represents a single partition entry in the MBR
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct SunxiPartitionRaw {
    pub addrhi: u32,
    pub addrlo: u32,
    pub lenhi: u32,
    pub lenlo: u32,
    pub classname: [u8; PART_NAME_MAX_LEN],
    pub name: [u8; PART_NAME_MAX_LEN],
    pub user_type: u32,
    pub keydata: u32,
    pub ro: u32,
    pub reserved: [u8; PART_SIZE_RES_LEN],
}

/// Size of a partition entry
pub const SUNXI_PARTITION_SIZE: usize = std::mem::size_of::<SunxiPartitionRaw>();

impl SunxiPartitionRaw {
    /// Parse partition entry from raw data
    pub fn parse(data: &[u8]) -> Result<&Self, &'static str> {
        if data.len() < SUNXI_PARTITION_SIZE {
            return Err("Data too short for Sunxi partition");
        }

        let ptr = data.as_ptr() as *const SunxiPartitionRaw;
        Ok(unsafe { &*ptr })
    }

    /// Get class name as string
    pub fn classname_str(&self) -> String {
        String::from_utf8_lossy(&self.classname)
            .trim_end_matches('\0')
            .to_string()
    }

    /// Get partition name as string
    pub fn name_str(&self) -> String {
        String::from_utf8_lossy(&self.name)
            .trim_end_matches('\0')
            .to_string()
    }

    /// Get partition starting address
    pub fn address(&self) -> u64 {
        ((self.addrhi as u64) << 32) | (self.addrlo as u64)
    }

    /// Get partition size
    pub fn length(&self) -> u64 {
        ((self.lenhi as u64) << 32) | (self.lenlo as u64)
    }

    /// Check if partition is read-only
    pub fn readonly(&self) -> bool {
        self.ro != 0
    }
}

/// Parsed partition entry
#[derive(Debug, Clone)]
pub struct SunxiPartition {
    pub addrhi: u32,
    pub addrlo: u32,
    pub lenhi: u32,
    pub lenlo: u32,
    pub classname: String,
    pub name: String,
    pub user_type: u32,
    pub keydata: u32,
    pub ro: u32,
}

impl SunxiPartition {
    /// Create from raw partition entry
    pub fn from_raw(raw: &SunxiPartitionRaw) -> Self {
        Self {
            addrhi: raw.addrhi,
            addrlo: raw.addrlo,
            lenhi: raw.lenhi,
            lenlo: raw.lenlo,
            classname: raw.classname_str(),
            name: raw.name_str(),
            user_type: raw.user_type,
            keydata: raw.keydata,
            ro: raw.ro,
        }
    }

    /// Get partition starting address
    pub fn address(&self) -> u64 {
        ((self.addrhi as u64) << 32) | (self.addrlo as u64)
    }

    /// Get partition size
    pub fn length(&self) -> u64 {
        ((self.lenhi as u64) << 32) | (self.lenlo as u64)
    }

    /// Check if partition is read-only
    pub fn readonly(&self) -> bool {
        self.ro != 0
    }
}

/// Raw MBR structure
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct SunxiMbrRaw {
    pub crc32: u32,
    pub version: u32,
    pub magic: [u8; 8],
    pub copy: u32,
    pub index: u32,
    pub part_count: u32,
    pub stamp: u32,
    pub partitions: [SunxiPartitionRaw; MBR_MAX_PART_CNT],
}

impl SunxiMbrRaw {
    /// Parse MBR from raw data
    pub fn parse(data: &[u8]) -> Result<&Self, &'static str> {
        if data.len() < MBR_SIZE {
            return Err("Data too short for Sunxi MBR");
        }

        let ptr = data.as_ptr() as *const SunxiMbrRaw;
        let mbr = unsafe { &*ptr };

        let magic = String::from_utf8_lossy(&mbr.magic).to_string();
        if magic != MBR_MAGIC {
            return Err("Invalid MBR magic");
        }

        Ok(mbr)
    }

    /// Get magic string
    pub fn magic_str(&self) -> String {
        String::from_utf8_lossy(&self.magic).to_string()
    }
}

/// Parsed MBR structure
#[derive(Debug, Clone)]
pub struct SunxiMbr {
    pub crc32: u32,
    pub version: u32,
    pub magic: String,
    pub copy: u32,
    pub index: u32,
    pub part_count: u32,
    pub stamp: u32,
    pub partitions: Vec<SunxiPartition>,
}

impl SunxiMbr {
    /// Parse MBR from raw data
    pub fn parse(data: &[u8]) -> Result<Self, &'static str> {
        let raw = SunxiMbrRaw::parse(data)?;

        if raw.part_count as usize > MBR_MAX_PART_CNT {
            return Err("Partition count exceeds MBR capacity");
        }

        let mut partitions = Vec::with_capacity(raw.part_count as usize);
        for i in 0..raw.part_count as usize {
            let partition = SunxiPartition::from_raw(&raw.partitions[i]);
            partitions.push(partition);
        }

        Ok(Self {
            crc32: raw.crc32,
            version: raw.version,
            magic: raw.magic_str(),
            copy: raw.copy,
            index: raw.index,
            part_count: raw.part_count,
            stamp: raw.stamp,
            partitions,
        })
    }

    /// Convert to MbrInfo
    pub fn to_mbr_info(&self) -> MbrInfo {
        MbrInfo {
            part_count: self.part_count,
            partitions: self.partitions.clone(),
        }
    }
}

/// MBR information container
#[derive(Debug, Clone)]
pub struct MbrInfo {
    pub part_count: u32,
    pub partitions: Vec<SunxiPartition>,
}

/// Check if data contains a valid MBR
pub fn is_valid_mbr(data: &[u8]) -> bool {
    if data.len() < std::mem::size_of::<SunxiMbrRaw>() {
        return false;
    }

    let raw = SunxiMbrRaw::parse(data);
    raw.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::mbr_bytes;

    #[test]
    fn raw_parsers_reject_short_and_invalid_data() {
        assert_eq!(
            SunxiPartitionRaw::parse(&[]).unwrap_err(),
            "Data too short for Sunxi partition"
        );
        assert_eq!(
            SunxiMbrRaw::parse(&[]).unwrap_err(),
            "Data too short for Sunxi MBR"
        );

        let mut invalid = vec![0u8; MBR_SIZE];
        invalid[8..16].copy_from_slice(b"not-mbr!");
        assert_eq!(
            SunxiMbrRaw::parse(&invalid).unwrap_err(),
            "Invalid MBR magic"
        );
        assert!(!is_valid_mbr(&invalid));
        assert!(!is_valid_mbr(&[]));
    }

    #[test]
    fn parses_partition_fields_and_high_words() {
        let address = 0x0000_0002_1234_5678;
        let length = 0x0000_0003_8765_4321;
        let bytes = mbr_bytes(&[("boot", address, length, true)]);
        let raw = SunxiMbrRaw::parse(&bytes).unwrap();
        assert_eq!(raw.magic_str(), MBR_MAGIC);

        let raw_partition = &raw.partitions[0];
        assert_eq!(raw_partition.classname_str(), "DISK");
        assert_eq!(raw_partition.name_str(), "boot");
        assert_eq!(raw_partition.address(), address);
        assert_eq!(raw_partition.length(), length);
        assert!(raw_partition.readonly());

        let partition_offset = std::mem::offset_of!(SunxiMbrRaw, partitions);
        let mut offset_bytes = vec![0xff];
        offset_bytes
            .extend_from_slice(&bytes[partition_offset..partition_offset + SUNXI_PARTITION_SIZE]);
        let parsed_from_offset = SunxiPartitionRaw::parse(&offset_bytes[1..]).unwrap();
        assert_eq!(parsed_from_offset.name_str(), "boot");
        assert_eq!(parsed_from_offset.address(), address);
        assert_eq!(parsed_from_offset.length(), length);

        let partition = SunxiPartition::from_raw(raw_partition);
        assert_eq!(partition.address(), address);
        assert_eq!(partition.length(), length);
        assert!(partition.readonly());
        assert_eq!(partition.classname, "DISK");
        assert_eq!(partition.name, "boot");
    }

    #[test]
    fn parses_mbr_and_builds_independent_info() {
        let bytes = mbr_bytes(&[
            ("boot", 0x8000, 0x1000, false),
            ("rootfs", 0x9000, 0x2000, true),
        ]);
        assert!(is_valid_mbr(&bytes));

        let mbr = SunxiMbr::parse(&bytes).unwrap();
        assert_eq!(mbr.part_count, 2);
        assert_eq!(mbr.partitions.len(), 2);
        assert_eq!(mbr.magic, MBR_MAGIC);
        assert!(!mbr.partitions[0].readonly());
        assert!(mbr.partitions[1].readonly());

        let mut info = mbr.to_mbr_info();
        info.partitions[0].name = "changed".to_string();
        assert_eq!(mbr.partitions[0].name, "boot");
    }

    #[test]
    fn rejects_partition_count_larger_than_fixed_table() {
        let mut bytes = mbr_bytes(&[]);
        bytes[24..28].copy_from_slice(&((MBR_MAX_PART_CNT as u32) + 1).to_le_bytes());
        assert_eq!(
            SunxiMbr::parse(&bytes).unwrap_err(),
            "Partition count exceeds MBR capacity"
        );
    }
}
