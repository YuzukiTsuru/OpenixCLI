#![allow(dead_code)]

pub const IMAGEWTY_MAGIC: &str = "IMAGEWTY";
pub const IMAGEWTY_MAGIC_LEN: usize = 8;
pub const IMAGEWTY_FILEHDR_LEN: usize = 1024;
pub const IMAGEWTY_FHDR_MAINTYPE_LEN: usize = 8;
pub const IMAGEWTY_FHDR_SUBTYPE_LEN: usize = 16;
pub const IMAGEWTY_FHDR_FILENAME_LEN: usize = 256;

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct ImageHeaderV1 {
    pub pid: u32,
    pub vid: u32,
    pub hardware_id: u32,
    pub firmware_id: u32,
    pub val1: u32,
    pub val1024: u32,
    pub num_files: u32,
    pub val1024_2: u32,
    pub val0: u32,
    pub val0_2: u32,
    pub val0_3: u32,
    pub val0_4: u32,
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct ImageHeaderV3 {
    pub unknown: u32,
    pub pid: u32,
    pub vid: u32,
    pub hardware_id: u32,
    pub firmware_id: u32,
    pub val1: u32,
    pub val1024: u32,
    pub num_files: u32,
    pub val1024_2: u32,
    pub val0: u32,
    pub val0_2: u32,
    pub val0_3: u32,
    pub val0_4: u32,
}

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

#[repr(C, packed)]
pub struct ImageHeader {
    pub magic: [u8; IMAGEWTY_MAGIC_LEN],
    pub header_version: u32,
    pub header_size: u32,
    pub ram_base: u32,
    pub version: u32,
    pub image_size: u32,
    pub image_header_size: u32,
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
        let ram_base = self.ram_base;
        let version = self.version;
        let image_size = self.image_size;
        let image_header_size = self.image_header_size;
        f.debug_struct("ImageHeader")
            .field("magic", &self.magic_str())
            .field("header_version", &header_version)
            .field("header_size", &header_size)
            .field("ram_base", &ram_base)
            .field("version", &version)
            .field("image_size", &image_size)
            .field("image_header_size", &image_header_size)
            .finish()
    }
}

impl ImageHeader {
    pub fn parse(data: &[u8]) -> Result<&Self, &'static str> {
        if data.len() < std::mem::size_of::<ImageHeader>() {
            return Err("Data too short for ImageHeader");
        }

        let ptr = data.as_ptr() as *const ImageHeader;
        Ok(unsafe { &*ptr })
    }

    pub fn parse_mut(data: &mut [u8]) -> Result<&mut Self, &'static str> {
        if data.len() < std::mem::size_of::<ImageHeader>() {
            return Err("Data too short for ImageHeader");
        }

        let ptr = data.as_mut_ptr() as *mut ImageHeader;
        Ok(unsafe { &mut *ptr })
    }

    pub fn magic_str(&self) -> String {
        String::from_utf8_lossy(&self.magic).to_string()
    }

    pub fn num_files(&self) -> u32 {
        unsafe {
            if self.header_version == 0x0300 {
                self.data.v3.num_files
            } else {
                self.data.v1.num_files
            }
        }
    }

    pub fn pid(&self) -> u32 {
        unsafe {
            if self.header_version == 0x0300 {
                self.data.v3.pid
            } else {
                self.data.v1.pid
            }
        }
    }

    pub fn vid(&self) -> u32 {
        unsafe {
            if self.header_version == 0x0300 {
                self.data.v3.vid
            } else {
                self.data.v1.vid
            }
        }
    }

    pub fn hardware_id(&self) -> u32 {
        unsafe {
            if self.header_version == 0x0300 {
                self.data.v3.hardware_id
            } else {
                self.data.v1.hardware_id
            }
        }
    }

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

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct FileHeaderV1 {
    pub unknown_3: u32,
    pub stored_length: u32,
    pub original_length: u32,
    pub offset: u32,
    pub unknown: u32,
    pub filename: [u8; IMAGEWTY_FHDR_FILENAME_LEN],
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct FileHeaderV3 {
    pub unknown_0: u32,
    pub filename: [u8; IMAGEWTY_FHDR_FILENAME_LEN],
    pub stored_length: u32,
    pub pad1: u32,
    pub original_length: u32,
    pub pad2: u32,
    pub offset: u32,
}

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
    pub fn parse(data: &[u8]) -> Result<&Self, &'static str> {
        if data.len() < std::mem::size_of::<FileHeader>() {
            return Err("Data too short for FileHeader");
        }

        let ptr = data.as_ptr() as *const FileHeader;
        Ok(unsafe { &*ptr })
    }

    pub fn parse_mut(data: &mut [u8]) -> Result<&mut Self, &'static str> {
        if data.len() < std::mem::size_of::<FileHeader>() {
            return Err("Data too short for FileHeader");
        }

        let ptr = data.as_mut_ptr() as *mut FileHeader;
        Ok(unsafe { &mut *ptr })
    }

    pub fn maintype_str(&self) -> String {
        let s = String::from_utf8_lossy(&self.maintype).to_string();
        s.trim_end_matches(['\0', ' ']).to_string()
    }

    pub fn subtype_str(&self) -> String {
        let s = String::from_utf8_lossy(&self.subtype).to_string();
        s.trim_end_matches(['\0', ' ']).to_string()
    }

    pub fn stored_length(&self, header_version: u32) -> u32 {
        unsafe {
            if header_version == 0x0300 {
                self.data.v3.stored_length
            } else {
                self.data.v1.stored_length
            }
        }
    }

    pub fn original_length(&self, header_version: u32) -> u32 {
        unsafe {
            if header_version == 0x0300 {
                self.data.v3.original_length
            } else {
                self.data.v1.original_length
            }
        }
    }

    pub fn offset(&self, header_version: u32) -> u32 {
        unsafe {
            if header_version == 0x0300 {
                self.data.v3.offset
            } else {
                self.data.v1.offset
            }
        }
    }

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

#[derive(Debug, Clone)]
pub struct ImageInfo {
    pub header: ImageHeader,
    pub files: Vec<FileInfo>,
    pub is_encrypted: bool,
    pub image_size: u32,
    pub num_files: u32,
}

#[derive(Debug, Clone)]
pub struct FileInfo {
    pub filename: String,
    pub maintype: String,
    pub subtype: String,
    pub stored_length: u32,
    pub original_length: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageType {
    Nand = 0,
    Sdcard = 1,
    Emmc = 2,
    Spinor = 3,
    Emmc3 = 4,
    Spinand = 5,
    Sd1 = 6,
    Emmc0 = 7,
    Ufs = 8,
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
