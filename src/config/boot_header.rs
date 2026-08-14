//! Boot header definitions and parsers
//!
//! Provides structures and parsers for Allwinner boot headers:
//! - Boot0 header: First stage bootloader
//! - U-Boot header: Second stage bootloader
//! - GPIO configurations

#![allow(dead_code)]

/// Magic string for Boot0 header
pub const BOOT0_MAGIC: &str = "eGON.BT0";
/// Magic string for U-Boot header
pub const UBOOT_MAGIC: &str = "uboot";

/// Boot0 header structure
///
/// This is the first stage bootloader header for Allwinner chips.
/// It contains initialization code and parameters for DRAM and other hardware.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Boot0Header {
    pub jump_instruction: u32,
    pub magic: [u8; 8],
    pub check_sum: u32,
    pub length: u32,
    pub pub_head_size: u32,
    pub pub_head_vsn: [u8; 4],
    pub ret_addr: u32,
    pub run_addr: u32,
    pub boot_cpu: u32,
    pub platform: [u8; 8],
}

impl Boot0Header {
    /// Parse Boot0 header from raw data
    pub fn parse(data: &[u8]) -> Result<&Self, &'static str> {
        if data.len() < std::mem::size_of::<Boot0Header>() {
            return Err("Data too short for Boot0 header");
        }

        let ptr = data.as_ptr() as *const Boot0Header;
        Ok(unsafe { &*ptr })
    }

    /// Parse Boot0 header from mutable raw data
    pub fn parse_mut(data: &mut [u8]) -> Result<&mut Self, &'static str> {
        if data.len() < std::mem::size_of::<Boot0Header>() {
            return Err("Data too short for Boot0 header");
        }

        let ptr = data.as_mut_ptr() as *mut Boot0Header;
        Ok(unsafe { &mut *ptr })
    }

    /// Get magic string from header
    pub fn magic_str(&self) -> String {
        String::from_utf8_lossy(&self.magic).to_string()
    }

    /// Get platform string from header
    pub fn platform_str(&self) -> String {
        String::from_utf8_lossy(&self.platform).to_string()
    }
}

/// U-Boot base header structure
///
/// Contains basic information about the U-Boot image
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct UBootBaseHeader {
    pub jump_instruction: u32,
    pub magic: [u8; 8],
    pub check_sum: u32,
    pub align_size: u32,
    pub length: u32,
    pub uboot_length: u32,
    pub version: [u8; 8],
    pub platform: [u8; 8],
    pub run_addr: u32,
}

impl UBootBaseHeader {
    /// Parse U-Boot base header from raw data
    pub fn parse(data: &[u8]) -> Result<&Self, &'static str> {
        if data.len() < std::mem::size_of::<UBootBaseHeader>() {
            return Err("Data too short for U-Boot base header");
        }

        let ptr = data.as_ptr() as *const UBootBaseHeader;
        Ok(unsafe { &*ptr })
    }

    /// Parse U-Boot base header from mutable raw data
    pub fn parse_mut(data: &mut [u8]) -> Result<&mut Self, &'static str> {
        if data.len() < std::mem::size_of::<UBootBaseHeader>() {
            return Err("Data too short for U-Boot base header");
        }

        let ptr = data.as_mut_ptr() as *mut UBootBaseHeader;
        Ok(unsafe { &mut *ptr })
    }

    /// Get magic string from header
    pub fn magic_str(&self) -> String {
        String::from_utf8_lossy(&self.magic).to_string()
    }

    /// Get version string from header
    pub fn version_str(&self) -> String {
        String::from_utf8_lossy(&self.version).to_string()
    }

    /// Get platform string from header
    pub fn platform_str(&self) -> String {
        String::from_utf8_lossy(&self.platform).to_string()
    }
}

/// GPIO configuration structure for U-Boot
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct UBootNormalGpioCfg {
    pub port: u8,
    pub port_num: u8,
    pub mul_sel: u8,
    pub pull: u8,
    pub drv_level: u8,
    pub data: u8,
    pub reserved: [u8; 2],
}

impl UBootNormalGpioCfg {
    /// Parse GPIO configuration from raw data
    pub fn parse(data: &[u8]) -> Result<&Self, &'static str> {
        if data.len() < std::mem::size_of::<UBootNormalGpioCfg>() {
            return Err("Data too short for GPIO config");
        }

        let ptr = data.as_ptr() as *const UBootNormalGpioCfg;
        Ok(unsafe { &*ptr })
    }
}

/// U-Boot data header structure
///
/// Contains DRAM parameters and other hardware initialization data
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct UBootDataHeader {
    pub dram_para: [u32; 32],
    pub run_clock: i32,
    pub run_core_vol: i32,
    pub uart_port: i32,
    pub uart_gpio: [UBootNormalGpioCfg; 2],
    pub twi_port: i32,
    pub twi_gpio: [UBootNormalGpioCfg; 2],
    pub work_mode: i32,
    pub storage_type: i32,
}

impl UBootDataHeader {
    /// Parse U-Boot data header from raw data
    pub fn parse(data: &[u8]) -> Result<&Self, &'static str> {
        if data.len() < std::mem::size_of::<UBootDataHeader>() {
            return Err("Data too short for U-Boot data header");
        }

        let ptr = data.as_ptr() as *const UBootDataHeader;
        Ok(unsafe { &*ptr })
    }

    /// Parse U-Boot data header from mutable raw data
    pub fn parse_mut(data: &mut [u8]) -> Result<&mut Self, &'static str> {
        if data.len() < std::mem::size_of::<UBootDataHeader>() {
            return Err("Data too short for U-Boot data header");
        }

        let ptr = data.as_mut_ptr() as *mut UBootDataHeader;
        Ok(unsafe { &mut *ptr })
    }

    /// Set work mode in the header
    pub fn set_work_mode(data: &mut [u8], mode: u32) {
        if let Ok(header) = Self::parse_mut(data) {
            header.work_mode = mode as i32;
        }
    }
}

/// Complete U-Boot header structure
///
/// Combines base header and data header
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct UBootHeader {
    pub uboot_head: UBootBaseHeader,
    pub uboot_data: UBootDataHeader,
}

impl UBootHeader {
    /// Parse U-Boot header from raw data
    pub fn parse(data: &[u8]) -> Result<&Self, &'static str> {
        if data.len() < std::mem::size_of::<UBootHeader>() {
            return Err("Data too short for U-Boot header");
        }

        let ptr = data.as_ptr() as *const UBootHeader;
        Ok(unsafe { &*ptr })
    }

    /// Parse U-Boot header from mutable raw data
    pub fn parse_mut(data: &mut [u8]) -> Result<&mut Self, &'static str> {
        if data.len() < std::mem::size_of::<UBootHeader>() {
            return Err("Data too short for U-Boot header");
        }

        let ptr = data.as_mut_ptr() as *mut UBootHeader;
        Ok(unsafe { &mut *ptr })
    }

    /// Set work mode in the header
    pub fn set_work_mode(data: &mut [u8], mode: u32) {
        let data_offset = std::mem::size_of::<UBootBaseHeader>();
        if data.len() < data_offset {
            return;
        }
        UBootDataHeader::set_work_mode(&mut data[data_offset..], mode);
    }
}

/// Work mode: USB product mode
pub const WORK_MODE_USB_PRODUCT: u32 = 0x10;

/// Boot file mode: Normal boot
pub const BOOT_FILE_MODE_NORMAL: u32 = 0;
/// Boot file mode: TOC boot
pub const BOOT_FILE_MODE_TOC: u32 = 1;
/// Boot file mode: Reserved 0
pub const BOOT_FILE_MODE_RESERVED0: u32 = 2;
/// Boot file mode: Reserved 1
pub const BOOT_FILE_MODE_RESERVED1: u32 = 3;
/// Boot file mode: Package
pub const BOOT_FILE_MODE_PKG: u32 = 4;

/// Get human-readable string for boot file mode
pub fn get_sunxi_boot_file_mode_string(mode: u32) -> &'static str {
    match mode {
        BOOT_FILE_MODE_NORMAL => "Normal Boot File",
        BOOT_FILE_MODE_TOC => "TOC Boot File",
        BOOT_FILE_MODE_RESERVED0 => "Reserved Boot File 0",
        BOOT_FILE_MODE_RESERVED1 => "Reserved Boot File 1",
        BOOT_FILE_MODE_PKG => "Boot Package File",
        _ => "Unknown Boot File Type",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsers_reject_short_buffers() {
        assert!(Boot0Header::parse(&[]).is_err());
        assert!(Boot0Header::parse_mut(&mut []).is_err());
        assert!(UBootBaseHeader::parse(&[]).is_err());
        assert!(UBootBaseHeader::parse_mut(&mut []).is_err());
        assert!(UBootNormalGpioCfg::parse(&[]).is_err());
        assert!(UBootDataHeader::parse(&[]).is_err());
        assert!(UBootDataHeader::parse_mut(&mut []).is_err());
        assert!(UBootHeader::parse(&[]).is_err());
        assert!(UBootHeader::parse_mut(&mut []).is_err());
    }

    #[test]
    fn boot0_mutation_and_string_accessors_round_trip() {
        let mut bytes = vec![0u8; std::mem::size_of::<Boot0Header>()];
        let header = Boot0Header::parse_mut(&mut bytes).unwrap();
        header.magic = *b"eGON.BT0";
        header.platform = *b"sun50iw9";

        let parsed = Boot0Header::parse(&bytes).unwrap();
        assert_eq!(parsed.magic_str(), "eGON.BT0");
        assert_eq!(parsed.platform_str(), "sun50iw9");
    }

    #[test]
    fn uboot_headers_mutate_and_report_strings() {
        let mut base_bytes = vec![0u8; std::mem::size_of::<UBootBaseHeader>()];
        let base = UBootBaseHeader::parse_mut(&mut base_bytes).unwrap();
        base.magic = *b"uboot\0\0\0";
        base.version = *b"v1.2.3\0\0";
        base.platform = *b"sun55iw3";
        let base = UBootBaseHeader::parse(&base_bytes).unwrap();
        assert_eq!(base.magic_str(), "uboot\0\0\0");
        assert_eq!(base.version_str(), "v1.2.3\0\0");
        assert_eq!(base.platform_str(), "sun55iw3");

        let mut gpio_bytes = vec![0u8; std::mem::size_of::<UBootNormalGpioCfg>()];
        gpio_bytes[0] = 3;
        let gpio = UBootNormalGpioCfg::parse(&gpio_bytes).unwrap();
        assert_eq!(gpio.port, 3);
    }

    #[test]
    fn work_mode_setters_accept_full_and_short_buffers() {
        let mut data_bytes = vec![0u8; std::mem::size_of::<UBootDataHeader>()];
        UBootDataHeader::set_work_mode(&mut data_bytes, WORK_MODE_USB_PRODUCT);
        let data = UBootDataHeader::parse(&data_bytes).unwrap();
        let work_mode = data.work_mode;
        assert_eq!(work_mode, WORK_MODE_USB_PRODUCT as i32);

        let mut uboot_bytes = vec![0u8; std::mem::size_of::<UBootHeader>()];
        UBootHeader::set_work_mode(&mut uboot_bytes, 0x55);
        let uboot = UBootHeader::parse_mut(&mut uboot_bytes).unwrap();
        let work_mode = uboot.uboot_data.work_mode;
        assert_eq!(work_mode, 0x55);

        UBootHeader::set_work_mode(&mut [], 1);
        UBootHeader::set_work_mode(&mut [0; 1], 1);
    }

    #[test]
    fn boot_file_mode_names_cover_known_and_unknown_values() {
        assert_eq!(
            get_sunxi_boot_file_mode_string(BOOT_FILE_MODE_NORMAL),
            "Normal Boot File"
        );
        assert_eq!(
            get_sunxi_boot_file_mode_string(BOOT_FILE_MODE_TOC),
            "TOC Boot File"
        );
        assert_eq!(
            get_sunxi_boot_file_mode_string(BOOT_FILE_MODE_RESERVED0),
            "Reserved Boot File 0"
        );
        assert_eq!(
            get_sunxi_boot_file_mode_string(BOOT_FILE_MODE_RESERVED1),
            "Reserved Boot File 1"
        );
        assert_eq!(
            get_sunxi_boot_file_mode_string(BOOT_FILE_MODE_PKG),
            "Boot Package File"
        );
        assert_eq!(
            get_sunxi_boot_file_mode_string(u32::MAX),
            "Unknown Boot File Type"
        );
    }
}
