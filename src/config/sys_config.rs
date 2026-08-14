//! System configuration parser
//!
//! Provides parsers for system configuration data including DRAM parameters

#![allow(dead_code)]

use crate::firmware::StorageType;

/// DRAM parameter information structure
///
/// Contains DRAM initialization parameters
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct DramParamInfo {
    pub dram_init_flag: u32,
    pub dram_update_flag: u32,
    pub dram_para: [u32; 32],
}

impl DramParamInfo {
    /// Create empty DRAM parameter info
    pub fn create_empty() -> Self {
        Self {
            dram_init_flag: 0,
            dram_update_flag: 0,
            dram_para: [0u32; 32],
        }
    }

    /// Parse DRAM parameters from raw data
    pub fn parse(data: &[u8]) -> Result<&Self, &'static str> {
        if data.len() < std::mem::size_of::<DramParamInfo>() {
            return Err("Data too short for DRAM param");
        }

        let ptr = data.as_ptr() as *const DramParamInfo;
        Ok(unsafe { &*ptr })
    }

    /// Parse DRAM parameters from mutable raw data
    pub fn parse_mut(data: &mut [u8]) -> Result<&mut Self, &'static str> {
        if data.len() < std::mem::size_of::<DramParamInfo>() {
            return Err("Data too short for DRAM param");
        }

        let ptr = data.as_mut_ptr() as *mut DramParamInfo;
        Ok(unsafe { &mut *ptr })
    }

    /// Serialize DRAM parameters to bytes
    pub fn serialize(&self) -> Vec<u8> {
        let size = std::mem::size_of::<DramParamInfo>();
        let mut data = vec![0u8; size];
        unsafe {
            std::ptr::copy_nonoverlapping(
                self as *const DramParamInfo as *const u8,
                data.as_mut_ptr(),
                size,
            );
        }
        data
    }
}

/// System configuration parser
pub struct SysConfigParser;

impl SysConfigParser {
    /// Parse system configuration from raw data
    pub fn parse(data: &[u8]) -> SysConfig {
        SysConfig {
            storage_type: Self::get_storage_type(data),
        }
    }

    /// Get storage type from raw data
    fn get_storage_type(data: &[u8]) -> u32 {
        data.get(..4)
            .and_then(|bytes| bytes.try_into().ok())
            .map(u32::from_le_bytes)
            .unwrap_or(0)
    }

    /// Get storage type from numeric value
    pub fn get_storage_type_from_num(num: u32) -> StorageType {
        StorageType::from(num)
    }
}

/// System configuration data
#[derive(Debug, Clone)]
pub struct SysConfig {
    /// Storage type (NAND, eMMC, SD card, etc.)
    pub storage_type: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dram_parameters_round_trip_and_reject_short_buffers() {
        let mut info = DramParamInfo::create_empty();
        info.dram_init_flag = 1;
        info.dram_update_flag = 2;
        info.dram_para[31] = 0xfeed_beef;

        let mut bytes = info.serialize();
        let parsed = DramParamInfo::parse(&bytes).unwrap();
        let init_flag = parsed.dram_init_flag;
        let last_param = parsed.dram_para[31];
        assert_eq!(init_flag, 1);
        assert_eq!(last_param, 0xfeed_beef);

        DramParamInfo::parse_mut(&mut bytes)
            .unwrap()
            .dram_update_flag = 3;
        let parsed = DramParamInfo::parse(&bytes).unwrap();
        let update_flag = parsed.dram_update_flag;
        assert_eq!(update_flag, 3);
        assert!(DramParamInfo::parse(&[]).is_err());
        assert!(DramParamInfo::parse_mut(&mut []).is_err());
    }

    #[test]
    fn sys_config_reads_little_endian_without_alignment_requirements() {
        assert_eq!(SysConfigParser::parse(&[]).storage_type, 0);
        assert_eq!(SysConfigParser::parse(&[8, 0, 0, 0]).storage_type, 8);

        let unaligned = [0xff, 3, 0, 0, 0];
        assert_eq!(SysConfigParser::parse(&unaligned[1..]).storage_type, 3);
        assert_eq!(
            SysConfigParser::get_storage_type_from_num(8),
            StorageType::Ufs
        );
        assert_eq!(
            SysConfigParser::get_storage_type_from_num(u32::MAX),
            StorageType::Auto
        );
    }
}
