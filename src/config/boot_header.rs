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
/// Mirror of the Allwinner `struct spare_boot_data_head` that follows the
/// U-Boot base header. Holds DRAM parameters and other hardware init data,
/// storage GPIO pin maps, boot-mode flags, and OTA / secure-boot metadata.
///
/// Field order matches the C layout. The `challenge_offset` tail is one
/// `uint64_t` on 64-bit builds and `uint32_t` + `uint32_t` padding on 32-bit
/// builds; both occupy the same bytes, so it is modeled as a single `u64`.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct UBootDataHeader {
    pub dram_para: [u32; 32],
    /// CPU clock in MHz.
    pub run_clock: i32,
    /// CPU core voltage in mV.
    pub run_core_vol: i32,
    /// UART controller number.
    pub uart_port: i32,
    /// UART GPIO info.
    pub uart_gpio: [UBootNormalGpioCfg; 2],
    /// TWI controller number.
    pub twi_port: i32,
    /// TWI GPIO info.
    pub twi_gpio: [UBootNormalGpioCfg; 2],
    /// Boot mode: normal boot, USB burn, card burn.
    pub work_mode: i32,
    /// 0: NAND, 1: SD card, 2: SPI NOR.
    pub storage_type: i32,
    /// NAND GPIO info.
    pub nand_gpio: [UBootNormalGpioCfg; 32],
    /// NAND spare info.
    pub nand_spare_data: [u8; 256],
    /// SD card GPIO info.
    pub sdcard_gpio: [UBootNormalGpioCfg; 32],
    /// SD card spare info.
    pub sdcard_spare_data: [u8; 256],
    pub secureos_exist: u8,
    pub monitor_exist: u8,
    /// Bit mask of enabled functions, see `UBOOT_FUNC_MASK_EN`.
    pub func_mask: u8,
    pub res: [u8; 1],
    /// Used in OTA update.
    pub uboot_start_sector_in_mmc: u32,
    /// Device tree offset within U-Boot.
    pub dtb_offset: i32,
    /// Boot package size; boot0 passes this value.
    pub boot_package_size: i32,
    /// Real DRAM size detected at boot.
    pub dram_scan_size: u32,
    /// Reserved, keeps the structure aligned.
    pub reserved: [i32; 1],
    pub pmu_type: u16,
    pub uart_input: u16,
    pub key_input: u16,
    /// Updated by `update_uboot`.
    pub secure_mode: u8,
    /// Updated by `update_uboot`.
    pub debug_mode: u8,
    /// Challenge salt for security checks; do not use directly, use with salt.
    pub challenge_offset: u64,
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

    /// Set work mode in the header.
    ///
    /// Writes through the parsed structure so no manual field offset is needed.
    /// No-op when the buffer is too short to hold the full data header.
    pub fn set_work_mode(data: &mut [u8], mode: u32) {
        if let Ok(header) = Self::parse_mut(data) {
            header.work_mode = mode as i32;
        }
    }

    /// Read the secure-boot mode flag from the header.
    ///
    /// The field is set by `update_uboot`; a non-zero value indicates the
    /// U-Boot was built for secure boot. Returns `None` when the buffer is too
    /// short to hold the full data header.
    pub fn secure_mode(data: &[u8]) -> Option<u8> {
        Self::parse(data).ok().map(|header| header.secure_mode)
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

    /// Set work mode in the embedded data header.
    ///
    /// Writes through the parsed structure so no manual field offset is needed.
    /// No-op when the buffer is too short to hold the full header.
    pub fn set_work_mode(data: &mut [u8], mode: u32) {
        if let Ok(header) = Self::parse_mut(data) {
            header.uboot_data.work_mode = mode as i32;
        }
    }

    /// Read the secure-boot mode flag from the embedded data header.
    ///
    /// Returns `None` when the buffer is too short to hold the full header.
    pub fn secure_mode(data: &[u8]) -> Option<u8> {
        Self::parse(data)
            .ok()
            .map(|header| header.uboot_data.secure_mode)
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
    fn spare_boot_data_head_layout_matches_the_c_struct() {
        use std::mem::MaybeUninit;
        use std::ptr::addr_of;

        // normal_gpio_cfg = 6 control bytes + 2 reserved = 8 bytes.
        assert_eq!(std::mem::size_of::<UBootNormalGpioCfg>(), 8);
        // Full Allwinner spare_boot_data_head.
        assert_eq!(std::mem::size_of::<UBootDataHeader>(), 1248);
        // UBootBaseHeader (48) + UBootDataHeader (1248).
        assert_eq!(std::mem::size_of::<UBootHeader>(), 1296);

        macro_rules! assert_field_offset {
            ($header:expr, $field:ident, $expected:expr) => {
                assert_eq!(
                    unsafe { addr_of!((*$header).$field) as usize - $header as usize },
                    $expected,
                    "offset of {}",
                    stringify!($field)
                );
            };
        }

        let header = MaybeUninit::<UBootDataHeader>::uninit();
        let base = header.as_ptr();

        assert_field_offset!(base, dram_para, 0);
        assert_field_offset!(base, run_clock, 128);
        assert_field_offset!(base, uart_port, 136);
        assert_field_offset!(base, uart_gpio, 140);
        assert_field_offset!(base, twi_port, 156);
        assert_field_offset!(base, twi_gpio, 160);
        assert_field_offset!(base, work_mode, 176);
        assert_field_offset!(base, storage_type, 180);
        assert_field_offset!(base, nand_gpio, 184);
        assert_field_offset!(base, nand_spare_data, 440);
        assert_field_offset!(base, sdcard_gpio, 696);
        assert_field_offset!(base, sdcard_spare_data, 952);
        assert_field_offset!(base, secureos_exist, 1208);
        assert_field_offset!(base, uboot_start_sector_in_mmc, 1212);
        assert_field_offset!(base, dtb_offset, 1216);
        assert_field_offset!(base, boot_package_size, 1220);
        assert_field_offset!(base, dram_scan_size, 1224);
        assert_field_offset!(base, pmu_type, 1232);
        assert_field_offset!(base, secure_mode, 1238);
        assert_field_offset!(base, debug_mode, 1239);
        assert_field_offset!(base, challenge_offset, 1240);
    }

    #[test]
    fn secure_mode_reader_reads_the_flag_and_rejects_short_buffers() {
        let mut data_bytes = vec![0u8; std::mem::size_of::<UBootDataHeader>()];
        assert_eq!(UBootDataHeader::secure_mode(&data_bytes), Some(0));
        UBootDataHeader::parse_mut(&mut data_bytes)
            .unwrap()
            .secure_mode = 1;
        assert_eq!(UBootDataHeader::secure_mode(&data_bytes), Some(1));

        // Whole-image convenience reads the flag past the base header.
        let mut uboot = vec![0u8; std::mem::size_of::<UBootHeader>()];
        UBootHeader::parse_mut(&mut uboot)
            .unwrap()
            .uboot_data
            .secure_mode = 1;
        assert_eq!(UBootHeader::secure_mode(&uboot), Some(1));

        // Buffers too short to hold the header report None.
        assert_eq!(UBootDataHeader::secure_mode(&[]), None);
        assert_eq!(UBootHeader::secure_mode(&[0; 200]), None);
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
