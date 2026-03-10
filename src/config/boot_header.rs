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
