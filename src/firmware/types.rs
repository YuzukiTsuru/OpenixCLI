//! Firmware type definitions
//!
//! Defines structures for parsing Allwinner firmware file formats

#![allow(dead_code)]

/// Magic string for IMAGEWTY firmware format
pub const IMAGEWTY_MAGIC: &str = "IMAGEWTY";
/// Length of magic string
pub const IMAGEWTY_MAGIC_LEN: usize = 8;
/// File header length
pub const IMAGEWTY_FILEHDR_LEN: usize = 1024;
/// Main type field length in file header
pub const IMAGEWTY_FHDR_MAINTYPE_LEN: usize = 8;
/// Sub type field length in file header
pub const IMAGEWTY_FHDR_SUBTYPE_LEN: usize = 16;
/// Filename field length in file header
pub const IMAGEWTY_FHDR_FILENAME_LEN: usize = 256;

/// Image header version 1 structure
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct ImageHeaderV1 {
    pub image_size: u32,
    pub align: u32,
    pub pid: u32,
    pub vid: u32,
    pub hardware_id: u32,
    pub firmware_id: u32,
    pub file_attr: u32,
    pub file_size: u32,
    pub file_count: u32,
    pub file_offset: u32,
    pub attr: u32,
    pub ext_size: u32,
    pub ext_offset: u32,
    pub reverse: [u8; 4],
}

/// Image header version 3 structure
///
/// Version 3 stores the 64-bit `image_size` and `ext_offset` fields as split
/// `lo`/`hi` `u32` pairs, which allows representing images larger than 4 GiB.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct ImageHeaderV3 {
    pub image_size_lo: u32,
    pub image_size_hi: u32,
    pub align: u32,
    pub pid: u32,
    pub vid: u32,
    pub hardware_id: u32,
    pub firmware_id: u32,
    pub file_attr: u32,
    pub file_size: u32,
    pub file_count: u32,
    pub file_offset: u32,
    pub attr: u32,
    pub ext_size: u32,
    pub ext_offset_lo: u32,
    pub ext_offset_hi: u32,
    pub reverse: [u8; 12],
}

/// Union for different header versions
#[repr(C, packed)]
pub union ImageHeaderVersionData {
    pub v1: ImageHeaderV1,
    pub v3: ImageHeaderV3,
}

impl Clone for ImageHeaderVersionData {
    fn clone(&self) -> Self {
        *self
    }
}

impl Copy for ImageHeaderVersionData {}

impl std::fmt::Debug for ImageHeaderVersionData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageHeaderVersionData").finish()
    }
}

/// Main image header structure
///
/// Contains metadata about the firmware image
#[repr(C, packed)]
pub struct ImageHeader {
    pub magic: [u8; IMAGEWTY_MAGIC_LEN],
    pub header_version: u32,
    pub header_size: u32,
    pub attr: u32,
    pub version: u32,
    pub data: ImageHeaderVersionData,
}

impl Clone for ImageHeader {
    fn clone(&self) -> Self {
        *self
    }
}

impl Copy for ImageHeader {}

impl std::fmt::Debug for ImageHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let header_version = self.header_version;
        let header_size = self.header_size;
        let attr = self.attr;
        let version = self.version;
        f.debug_struct("ImageHeader")
            .field("magic", &self.magic_str())
            .field("header_version", &header_version)
            .field("header_size", &header_size)
            .field("attr", &attr)
            .field("version", &version)
            .finish()
    }
}

impl ImageHeader {
    /// Parse image header from raw data
    pub fn parse(data: &[u8]) -> Result<&Self, &'static str> {
        if data.len() < std::mem::size_of::<ImageHeader>() {
            return Err("Data too short for ImageHeader");
        }

        let ptr = data.as_ptr() as *const ImageHeader;
        Ok(unsafe { &*ptr })
    }

    /// Parse image header from mutable raw data
    pub fn parse_mut(data: &mut [u8]) -> Result<&mut Self, &'static str> {
        if data.len() < std::mem::size_of::<ImageHeader>() {
            return Err("Data too short for ImageHeader");
        }

        let ptr = data.as_mut_ptr() as *mut ImageHeader;
        Ok(unsafe { &mut *ptr })
    }

    /// Get magic string from header
    pub fn magic_str(&self) -> String {
        String::from_utf8_lossy(&self.magic).to_string()
    }

    /// Get number of files in the image
    pub fn num_files(&self) -> u32 {
        unsafe {
            if self.header_version == 0x0300 {
                self.data.v3.file_count
            } else {
                self.data.v1.file_count
            }
        }
    }

    /// Get total image size
    ///
    /// In v3 the size is stored as a split 64-bit `lo`/`hi` pair, which allows
    /// representing images larger than 4 GiB.
    pub fn image_size(&self) -> u64 {
        unsafe {
            if self.header_version == 0x0300 {
                let v3 = self.data.v3;
                (v3.image_size_lo as u64) | ((v3.image_size_hi as u64) << 32)
            } else {
                self.data.v1.image_size as u64
            }
        }
    }

    /// Get product ID
    pub fn pid(&self) -> u32 {
        unsafe {
            if self.header_version == 0x0300 {
                self.data.v3.pid
            } else {
                self.data.v1.pid
            }
        }
    }

    /// Get vendor ID
    pub fn vid(&self) -> u32 {
        unsafe {
            if self.header_version == 0x0300 {
                self.data.v3.vid
            } else {
                self.data.v1.vid
            }
        }
    }

    /// Get hardware ID
    pub fn hardware_id(&self) -> u32 {
        unsafe {
            if self.header_version == 0x0300 {
                self.data.v3.hardware_id
            } else {
                self.data.v1.hardware_id
            }
        }
    }

    /// Get firmware ID
    pub fn firmware_id(&self) -> u32 {
        unsafe {
            if self.header_version == 0x0300 {
                self.data.v3.firmware_id
            } else {
                self.data.v1.firmware_id
            }
        }
    }
}

/// File header version 1 structure
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct FileHeaderV1 {
    pub attr: u32,
    pub stored_length: u32,
    pub original_length: u32,
    pub offset: u32,
    pub checksum: u32,
    pub filename: [u8; IMAGEWTY_FHDR_FILENAME_LEN],
}

/// File header version 3 structure
///
/// Version 3 stores the 64-bit offset/length fields as split `lo`/`hi` `u32`
/// pairs, which allows representing files larger than 4 GiB.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct FileHeaderV3 {
    pub attr: u32,
    pub filename: [u8; IMAGEWTY_FHDR_FILENAME_LEN],
    pub stored_length_lo: u32,
    pub stored_length_hi: u32,
    pub original_length_lo: u32,
    pub original_length_hi: u32,
    pub offset_lo: u32,
    pub offset_hi: u32,
    pub unknown: [u8; 64],
    pub checksum: u32,
}

/// Union for different file header versions
#[repr(C, packed)]
pub union FileHeaderVersionData {
    pub v1: FileHeaderV1,
    pub v3: FileHeaderV3,
}

impl Clone for FileHeaderVersionData {
    fn clone(&self) -> Self {
        *self
    }
}

impl Copy for FileHeaderVersionData {}

impl std::fmt::Debug for FileHeaderVersionData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileHeaderVersionData").finish()
    }
}

/// File header structure
///
/// Contains metadata about a single file in the firmware image
#[repr(C, packed)]
pub struct FileHeader {
    pub filename_len: u32,
    pub total_header_size: u32,
    pub maintype: [u8; IMAGEWTY_FHDR_MAINTYPE_LEN],
    pub subtype: [u8; IMAGEWTY_FHDR_SUBTYPE_LEN],
    pub data: FileHeaderVersionData,
}

impl Clone for FileHeader {
    fn clone(&self) -> Self {
        *self
    }
}

impl Copy for FileHeader {}

impl std::fmt::Debug for FileHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let filename_len = self.filename_len;
        let total_header_size = self.total_header_size;
        f.debug_struct("FileHeader")
            .field("filename_len", &filename_len)
            .field("total_header_size", &total_header_size)
            .field("maintype", &self.maintype_str())
            .field("subtype", &self.subtype_str())
            .finish()
    }
}

impl FileHeader {
    /// Parse file header from raw data
    pub fn parse(data: &[u8]) -> Result<&Self, &'static str> {
        if data.len() < std::mem::size_of::<FileHeader>() {
            return Err("Data too short for FileHeader");
        }

        let ptr = data.as_ptr() as *const FileHeader;
        Ok(unsafe { &*ptr })
    }

    /// Parse file header from mutable raw data
    pub fn parse_mut(data: &mut [u8]) -> Result<&mut Self, &'static str> {
        if data.len() < std::mem::size_of::<FileHeader>() {
            return Err("Data too short for FileHeader");
        }

        let ptr = data.as_mut_ptr() as *mut FileHeader;
        Ok(unsafe { &mut *ptr })
    }

    /// Get main type as string
    pub fn maintype_str(&self) -> String {
        let s = String::from_utf8_lossy(&self.maintype).to_string();
        s.trim_end_matches(['\0', ' ']).to_string()
    }

    /// Get sub type as string
    pub fn subtype_str(&self) -> String {
        let s = String::from_utf8_lossy(&self.subtype).to_string();
        s.trim_end_matches(['\0', ' ']).to_string()
    }

    /// Get stored length (compressed size)
    pub fn stored_length(&self, header_version: u32) -> u64 {
        unsafe {
            if header_version == 0x0300 {
                let v3 = self.data.v3;
                (v3.stored_length_lo as u64) | ((v3.stored_length_hi as u64) << 32)
            } else {
                self.data.v1.stored_length as u64
            }
        }
    }

    /// Get original length (uncompressed size)
    pub fn original_length(&self, header_version: u32) -> u64 {
        unsafe {
            if header_version == 0x0300 {
                let v3 = self.data.v3;
                (v3.original_length_lo as u64) | ((v3.original_length_hi as u64) << 32)
            } else {
                self.data.v1.original_length as u64
            }
        }
    }

    /// Get offset in the firmware file
    pub fn offset(&self, header_version: u32) -> u64 {
        unsafe {
            if header_version == 0x0300 {
                let v3 = self.data.v3;
                (v3.offset_lo as u64) | ((v3.offset_hi as u64) << 32)
            } else {
                self.data.v1.offset as u64
            }
        }
    }

    /// Get filename as string
    pub fn filename_str(&self, header_version: u32) -> String {
        unsafe {
            let filename_bytes = if header_version == 0x0300 {
                &self.data.v3.filename
            } else {
                &self.data.v1.filename
            };
            let end = filename_bytes
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(filename_bytes.len());
            String::from_utf8_lossy(&filename_bytes[..end]).to_string()
        }
    }
}

/// Image information container
#[derive(Debug, Clone)]
pub struct ImageInfo {
    pub header: ImageHeader,
    pub files: Vec<FileInfo>,
    pub is_encrypted: bool,
    pub image_size: u64,
    pub num_files: u32,
}

/// File information structure
#[derive(Debug, Clone)]
pub struct FileInfo {
    pub filename: String,
    pub maintype: String,
    pub subtype: String,
    pub stored_length: u64,
    pub original_length: u64,
    pub offset: u64,
}

/// Storage type enumeration
///
/// Represents different types of storage devices supported by Allwinner chips
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageType {
    /// NAND flash
    Nand = 0,
    /// SD card
    Sdcard = 1,
    /// eMMC
    Emmc = 2,
    /// SPI NOR flash
    Spinor = 3,
    /// eMMC v3
    Emmc3 = 4,
    /// SPI NAND flash
    Spinand = 5,
    /// SD card slot 1
    Sd1 = 6,
    /// eMMC slot 0
    Emmc0 = 7,
    /// UFS
    Ufs = 8,
    /// Auto-detect
    Auto = -1,
}

impl From<i32> for StorageType {
    fn from(value: i32) -> Self {
        match value {
            0 => StorageType::Nand,
            1 => StorageType::Sdcard,
            2 => StorageType::Emmc,
            3 => StorageType::Spinor,
            4 => StorageType::Emmc3,
            5 => StorageType::Spinand,
            6 => StorageType::Sd1,
            7 => StorageType::Emmc0,
            8 => StorageType::Ufs,
            _ => StorageType::Auto,
        }
    }
}

impl From<u32> for StorageType {
    fn from(value: u32) -> Self {
        StorageType::from(value as i32)
    }
}

impl std::fmt::Display for StorageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageType::Auto => write!(f, "Auto"),
            StorageType::Nand => write!(f, "NAND"),
            StorageType::Spinand => write!(f, "SPI NAND"),
            StorageType::Spinor => write!(f, "SPI NOR"),
            StorageType::Sdcard => write!(f, "SD Card"),
            StorageType::Emmc => write!(f, "eMMC"),
            StorageType::Emmc3 => write!(f, "eMMC3"),
            StorageType::Emmc0 => write!(f, "eMMC0"),
            StorageType::Sd1 => write!(f, "SD1"),
            StorageType::Ufs => write!(f, "UFS"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{fixed_bytes, write_struct};

    fn image_header_v1() -> ImageHeader {
        ImageHeader {
            magic: fixed_bytes::<IMAGEWTY_MAGIC_LEN>(IMAGEWTY_MAGIC.as_bytes()),
            header_version: 0x0100,
            header_size: IMAGEWTY_FILEHDR_LEN as u32,
            attr: 1,
            version: 2,
            data: ImageHeaderVersionData {
                v1: ImageHeaderV1 {
                    image_size: 123,
                    align: 1024,
                    pid: 1,
                    vid: 2,
                    hardware_id: 3,
                    firmware_id: 4,
                    file_attr: 0,
                    file_size: 123,
                    file_count: 5,
                    file_offset: 1024,
                    attr: 0,
                    ext_size: 0,
                    ext_offset: 0,
                    reverse: [0; 4],
                },
            },
        }
    }

    fn image_header_v3() -> ImageHeader {
        ImageHeader {
            magic: fixed_bytes::<IMAGEWTY_MAGIC_LEN>(IMAGEWTY_MAGIC.as_bytes()),
            header_version: 0x0300,
            header_size: IMAGEWTY_FILEHDR_LEN as u32,
            attr: 0,
            version: 3,
            data: ImageHeaderVersionData {
                v3: ImageHeaderV3 {
                    image_size_lo: 0x89ab_cdef,
                    image_size_hi: 1,
                    align: 1024,
                    pid: 11,
                    vid: 12,
                    hardware_id: 13,
                    firmware_id: 14,
                    file_attr: 0,
                    file_size: 0,
                    file_count: 15,
                    file_offset: 1024,
                    attr: 0,
                    ext_size: 0,
                    ext_offset_lo: 0,
                    ext_offset_hi: 0,
                    reverse: [0; 12],
                },
            },
        }
    }

    fn file_header_v1() -> FileHeader {
        FileHeader {
            filename_len: 8,
            total_header_size: IMAGEWTY_FILEHDR_LEN as u32,
            maintype: fixed_bytes::<IMAGEWTY_FHDR_MAINTYPE_LEN>(b"COMMON  "),
            subtype: fixed_bytes::<IMAGEWTY_FHDR_SUBTYPE_LEN>(b"SUBTYPE"),
            data: FileHeaderVersionData {
                v1: FileHeaderV1 {
                    attr: 0,
                    stored_length: 10,
                    original_length: 20,
                    offset: 30,
                    checksum: 0,
                    filename: fixed_bytes::<IMAGEWTY_FHDR_FILENAME_LEN>(b"file.bin"),
                },
            },
        }
    }

    fn file_header_v3() -> FileHeader {
        FileHeader {
            filename_len: 8,
            total_header_size: IMAGEWTY_FILEHDR_LEN as u32,
            maintype: fixed_bytes::<IMAGEWTY_FHDR_MAINTYPE_LEN>(b"RFSFAT16"),
            subtype: fixed_bytes::<IMAGEWTY_FHDR_SUBTYPE_LEN>(b"SYSTEM0000000000"),
            data: FileHeaderVersionData {
                v3: FileHeaderV3 {
                    attr: 0,
                    filename: fixed_bytes::<IMAGEWTY_FHDR_FILENAME_LEN>(b"large.img"),
                    stored_length_lo: 1,
                    stored_length_hi: 2,
                    original_length_lo: 3,
                    original_length_hi: 4,
                    offset_lo: 5,
                    offset_hi: 6,
                    unknown: [0; 64],
                    checksum: 0,
                },
            },
        }
    }

    #[test]
    fn image_header_v1_accessors_and_parsers_work() {
        assert!(ImageHeader::parse(&[]).is_err());
        assert!(ImageHeader::parse_mut(&mut []).is_err());

        let header = image_header_v1();
        let mut bytes = vec![0; std::mem::size_of::<ImageHeader>()];
        write_struct(&mut bytes, &header);
        ImageHeader::parse_mut(&mut bytes).unwrap().attr = 7;
        let parsed = ImageHeader::parse(&bytes).unwrap();
        assert_eq!(parsed.magic_str(), IMAGEWTY_MAGIC);
        assert_eq!(parsed.num_files(), 5);
        assert_eq!(parsed.image_size(), 123);
        assert_eq!(parsed.pid(), 1);
        assert_eq!(parsed.vid(), 2);
        assert_eq!(parsed.hardware_id(), 3);
        assert_eq!(parsed.firmware_id(), 4);
        assert!(format!("{parsed:?}").contains("IMAGEWTY"));
        assert_eq!(format!("{:?}", parsed.data), "ImageHeaderVersionData");
    }

    #[test]
    fn image_header_v3_combines_high_and_low_words() {
        let header = image_header_v3();
        assert_eq!(header.num_files(), 15);
        assert_eq!(header.image_size(), 0x0000_0001_89ab_cdef);
        assert_eq!(header.pid(), 11);
        assert_eq!(header.vid(), 12);
        assert_eq!(header.hardware_id(), 13);
        assert_eq!(header.firmware_id(), 14);
    }

    #[test]
    fn file_header_versions_expose_strings_lengths_and_offsets() {
        assert!(FileHeader::parse(&[]).is_err());
        assert!(FileHeader::parse_mut(&mut []).is_err());

        let header = file_header_v1();
        let mut bytes = vec![0; std::mem::size_of::<FileHeader>()];
        write_struct(&mut bytes, &header);
        FileHeader::parse_mut(&mut bytes).unwrap().filename_len = 9;
        let parsed = FileHeader::parse(&bytes).unwrap();
        assert_eq!(parsed.maintype_str(), "COMMON");
        assert_eq!(parsed.subtype_str(), "SUBTYPE");
        assert_eq!(parsed.stored_length(0x0100), 10);
        assert_eq!(parsed.original_length(0x0100), 20);
        assert_eq!(parsed.offset(0x0100), 30);
        assert_eq!(parsed.filename_str(0x0100), "file.bin");
        assert!(format!("{parsed:?}").contains("SUBTYPE"));
        assert_eq!(format!("{:?}", parsed.data), "FileHeaderVersionData");

        let v3 = file_header_v3();
        assert_eq!(v3.stored_length(0x0300), 0x0000_0002_0000_0001);
        assert_eq!(v3.original_length(0x0300), 0x0000_0004_0000_0003);
        assert_eq!(v3.offset(0x0300), 0x0000_0006_0000_0005);
        assert_eq!(v3.filename_str(0x0300), "large.img");
    }

    #[test]
    #[allow(clippy::clone_on_copy)]
    fn firmware_metadata_clones_are_independent_values() {
        let header = image_header_v1();
        let header_data = header.data.clone();
        assert_eq!(unsafe { header_data.v1.pid }, 1);
        let header_clone = header.clone();
        assert_eq!(header_clone.pid(), header.pid());

        let file_header = file_header_v1();
        let file_data = file_header.data.clone();
        assert_eq!(unsafe { file_data.v1.original_length }, 20);
        let file_header_clone = file_header.clone();
        assert_eq!(file_header_clone.filename_str(0x0100), "file.bin");

        let file_info = FileInfo {
            filename: "boot.img".into(),
            maintype: "RFSFAT16".into(),
            subtype: "BOOT_IMG00000000".into(),
            stored_length: 10,
            original_length: 20,
            offset: 30,
        };
        let image_info = ImageInfo {
            header,
            files: vec![file_info],
            is_encrypted: false,
            image_size: 40,
            num_files: 1,
        };
        let mut cloned = image_info.clone();
        cloned.files[0].filename = "changed.img".into();
        assert_eq!(image_info.files[0].filename, "boot.img");
        assert_eq!(cloned.files[0].filename, "changed.img");
    }

    #[test]
    fn storage_type_conversions_and_display_cover_every_variant() {
        let expected = [
            (0, StorageType::Nand, "NAND"),
            (1, StorageType::Sdcard, "SD Card"),
            (2, StorageType::Emmc, "eMMC"),
            (3, StorageType::Spinor, "SPI NOR"),
            (4, StorageType::Emmc3, "eMMC3"),
            (5, StorageType::Spinand, "SPI NAND"),
            (6, StorageType::Sd1, "SD1"),
            (7, StorageType::Emmc0, "eMMC0"),
            (8, StorageType::Ufs, "UFS"),
        ];
        for (number, storage, name) in expected {
            assert_eq!(StorageType::from(number), storage);
            assert_eq!(StorageType::from(number as u32), storage);
            assert_eq!(storage.to_string(), name);
        }
        assert_eq!(StorageType::from(-1), StorageType::Auto);
        assert_eq!(StorageType::from(9), StorageType::Auto);
        assert_eq!(StorageType::Auto.to_string(), "Auto");
    }
}
