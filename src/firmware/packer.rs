//! Firmware packer implementation
//!
//! Provides functionality for loading and parsing Allwinner firmware files (.fex)

#![allow(dead_code)]

use crate::firmware::types::*;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Suffix appended to partition download file names
const PARTITION_DOWNLOADFILE_SUFFIX: &str = "0000000000";

/// Errors that can occur during firmware packing/unpacking
#[derive(Debug, thiserror::Error)]
pub enum PackerError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Invalid magic: expected IMAGEWTY, got {0}")]
    InvalidMagic(String),
    #[error("Encrypted firmware not supported")]
    EncryptedNotSupported,
    #[error("Unknown header version: {0}")]
    UnknownHeaderVersion(u32),
    #[error("File not found: {0}")]
    FileNotFound(String),
    #[error("Image not loaded")]
    ImageNotLoaded,
    #[error("Parse error: {0}")]
    ParseError(&'static str),
    #[error("Invalid data range: start={start}, length={length}, available={available}")]
    InvalidRange {
        start: u64,
        length: u64,
        available: u64,
    },
}

/// Firmware packer for Allwinner IMAGEWTY format
///
/// Provides methods to load and extract files from firmware images
pub struct OpenixPacker {
    file: Option<File>,
    image_header: Option<ImageHeader>,
    file_headers: Vec<FileHeader>,
    is_encrypted: bool,
    image_loaded: bool,
}

impl OpenixPacker {
    /// Create a new empty packer
    pub fn new() -> Self {
        Self {
            file: None,
            image_header: None,
            file_headers: Vec::new(),
            is_encrypted: false,
            image_loaded: false,
        }
    }

    /// Load firmware from file path
    ///
    /// # Arguments
    /// * `path` - Path to the firmware file
    ///
    /// # Returns
    /// Ok(()) on success, PackerError on failure
    pub fn load<P: AsRef<Path>>(&mut self, path: P) -> Result<(), PackerError> {
        self.file = None;
        self.image_header = None;
        self.file_headers.clear();
        self.is_encrypted = false;
        self.image_loaded = false;

        let mut file = File::open(path)?;
        let file_size = file.metadata()?.len();

        let mut magic_buf = [0u8; IMAGEWTY_MAGIC_LEN];
        file.read_exact(&mut magic_buf)?;
        let magic = String::from_utf8_lossy(&magic_buf).to_string();

        if magic != IMAGEWTY_MAGIC {
            self.is_encrypted = true;
            return Err(PackerError::EncryptedNotSupported);
        }

        file.seek(SeekFrom::Start(0))?;

        let mut header_buf = [0u8; IMAGEWTY_FILEHDR_LEN];
        file.read_exact(&mut header_buf)?;

        let image_header = ImageHeader::parse(&header_buf).map_err(PackerError::ParseError)?;
        if !matches!(image_header.header_version, 0x0100 | 0x0200 | 0x0300) {
            return Err(PackerError::UnknownHeaderVersion(
                image_header.header_version,
            ));
        }
        let num_files = image_header.num_files();
        let headers_size = u64::from(num_files)
            .checked_add(1)
            .and_then(|count| count.checked_mul(IMAGEWTY_FILEHDR_LEN as u64))
            .ok_or(PackerError::ParseError("File header count overflow"))?;
        if headers_size > file_size {
            return Err(PackerError::ParseError(
                "File header table exceeds firmware size",
            ));
        }

        let mut file_headers = Vec::with_capacity(num_files as usize);
        for i in 0..num_files {
            let offset = IMAGEWTY_FILEHDR_LEN + (i as usize) * IMAGEWTY_FILEHDR_LEN;
            file.seek(SeekFrom::Start(offset as u64))?;

            let mut file_header_buf = [0u8; IMAGEWTY_FILEHDR_LEN];
            file.read_exact(&mut file_header_buf)?;

            let file_header =
                FileHeader::parse(&file_header_buf).map_err(PackerError::ParseError)?;
            file_headers.push(*file_header);
        }

        self.file = Some(file);
        self.image_header = Some(*image_header);
        self.file_headers = file_headers;
        self.image_loaded = true;

        Ok(())
    }

    /// Check if image is loaded
    pub fn is_image_loaded(&self) -> bool {
        self.image_loaded
    }

    /// Check if firmware is encrypted
    pub fn is_encrypted(&self) -> bool {
        self.is_encrypted
    }

    /// Get image information
    pub fn get_image_info(&self) -> ImageInfo {
        let header = match self.image_header {
            Some(ref h) => *h,
            None => ImageHeader {
                magic: [0u8; IMAGEWTY_MAGIC_LEN],
                header_version: 0,
                header_size: 0,
                attr: 0,
                version: 0,
                data: ImageHeaderVersionData {
                    v1: ImageHeaderV1 {
                        image_size: 0,
                        align: 0,
                        pid: 0,
                        vid: 0,
                        hardware_id: 0,
                        firmware_id: 0,
                        file_attr: 0,
                        file_size: 0,
                        file_count: 0,
                        file_offset: 0,
                        attr: 0,
                        ext_size: 0,
                        ext_offset: 0,
                        reverse: [0u8; 4],
                    },
                },
            },
        };

        let header_version = header.header_version;
        let files: Vec<FileInfo> = self
            .file_headers
            .iter()
            .map(|fh| FileInfo {
                filename: fh.filename_str(header_version),
                maintype: fh.maintype_str(),
                subtype: fh.subtype_str(),
                stored_length: fh.stored_length(header_version),
                original_length: fh.original_length(header_version),
                offset: fh.offset(header_version),
            })
            .collect();

        ImageInfo {
            image_size: header.image_size(),
            num_files: header.num_files(),
            header,
            files,
            is_encrypted: self.is_encrypted,
        }
    }

    /// Get header version
    fn get_header_version(&self) -> u32 {
        self.image_header
            .as_ref()
            .map(|h| h.header_version)
            .unwrap_or(0)
    }

    /// Get file header by filename
    pub fn get_file_header_by_filename(&self, filename: &str) -> Option<&FileHeader> {
        let header_version = self.get_header_version();
        self.file_headers
            .iter()
            .find(|fh| fh.filename_str(header_version) == filename)
    }

    /// Get file header by main type and sub type
    pub fn get_file_header_by_maintype_subtype(
        &self,
        maintype: &str,
        subtype: &str,
    ) -> Option<&FileHeader> {
        self.file_headers
            .iter()
            .find(|fh| fh.maintype_str() == maintype && fh.subtype_str() == subtype)
    }

    /// Find file header by sub type (ignores main type)
    pub fn find_file_header_by_subtype(&self, subtype: &str) -> Option<&FileHeader> {
        self.file_headers
            .iter()
            .find(|fh| fh.subtype_str() == subtype)
    }

    /// Find file data by sub type (ignores main type)
    pub fn find_file_data_by_subtype(&mut self, subtype: &str) -> Result<Vec<u8>, PackerError> {
        if !self.image_loaded {
            return Err(PackerError::ImageNotLoaded);
        }

        let header_version = self.get_header_version();
        let file_header = self
            .find_file_header_by_subtype(subtype)
            .ok_or_else(|| PackerError::FileNotFound(subtype.to_string()))?;

        self.read_data_at_offset(
            file_header.offset(header_version),
            file_header.original_length(header_version),
        )
    }

    /// Get file data by filename
    pub fn get_file_data_by_filename(&mut self, filename: &str) -> Result<Vec<u8>, PackerError> {
        if !self.image_loaded {
            return Err(PackerError::ImageNotLoaded);
        }

        let header_version = self.get_header_version();
        let file_header = self
            .get_file_header_by_filename(filename)
            .ok_or_else(|| PackerError::FileNotFound(filename.to_string()))?;

        self.read_data_at_offset(
            file_header.offset(header_version),
            file_header.original_length(header_version),
        )
    }

    /// Get file data by main type and sub type
    pub fn get_file_data_by_maintype_subtype(
        &mut self,
        maintype: &str,
        subtype: &str,
    ) -> Result<Vec<u8>, PackerError> {
        if !self.image_loaded {
            return Err(PackerError::ImageNotLoaded);
        }

        let header_version = self.get_header_version();
        let file_header = self
            .get_file_header_by_maintype_subtype(maintype, subtype)
            .ok_or_else(|| PackerError::FileNotFound(format!("{}/{}", maintype, subtype)))?;

        self.read_data_at_offset(
            file_header.offset(header_version),
            file_header.original_length(header_version),
        )
    }

    /// Get file info by main type and sub type
    pub fn get_file_info_by_maintype_subtype(
        &self,
        maintype: &str,
        subtype: &str,
    ) -> Option<(u64, u64)> {
        if !self.image_loaded {
            return None;
        }

        let header_version = self.get_header_version();
        let file_header = self.get_file_header_by_maintype_subtype(maintype, subtype)?;
        Some((
            file_header.offset(header_version),
            file_header.original_length(header_version),
        ))
    }

    /// Get file info by filename
    pub fn get_file_info_by_filename(&self, filename: &str) -> Option<(u64, u64)> {
        if !self.image_loaded {
            return None;
        }

        let header_version = self.get_header_version();
        let file_header = self.get_file_header_by_filename(filename)?;
        Some((
            file_header.offset(header_version),
            file_header.original_length(header_version),
        ))
    }

    /// Read data at specified offset
    fn read_data_at_offset(&mut self, offset: u64, length: u64) -> Result<Vec<u8>, PackerError> {
        let file = self.file.as_mut().ok_or(PackerError::ImageNotLoaded)?;

        let end = offset
            .checked_add(length)
            .ok_or(PackerError::InvalidRange {
                start: offset,
                length,
                available: file.metadata()?.len(),
            })?;
        let file_size = file.metadata()?.len();
        if end > file_size {
            return Err(PackerError::InvalidRange {
                start: offset,
                length,
                available: file_size,
            });
        }
        let buffer_len = usize::try_from(length).map_err(|_| PackerError::InvalidRange {
            start: offset,
            length,
            available: file_size,
        })?;

        file.seek(SeekFrom::Start(offset))?;

        let mut buffer = vec![0u8; buffer_len];
        file.read_exact(&mut buffer)?;

        Ok(buffer)
    }

    /// Get file data range by main type and sub type
    pub fn get_file_data_range_by_maintype_subtype(
        &mut self,
        maintype: &str,
        subtype: &str,
        start: u64,
        length: u64,
    ) -> Result<Vec<u8>, PackerError> {
        if !self.image_loaded {
            return Err(PackerError::ImageNotLoaded);
        }

        let header_version = self.get_header_version();
        let file_header = self
            .get_file_header_by_maintype_subtype(maintype, subtype)
            .ok_or_else(|| PackerError::FileNotFound(format!("{}/{}", maintype, subtype)))?;

        let original_length = file_header.original_length(header_version);
        let end = start.checked_add(length).ok_or(PackerError::InvalidRange {
            start,
            length,
            available: original_length,
        })?;
        if end > original_length {
            return Err(PackerError::InvalidRange {
                start,
                length,
                available: original_length,
            });
        }

        let absolute_start = file_header
            .offset(header_version)
            .checked_add(start)
            .ok_or(PackerError::InvalidRange {
                start,
                length,
                available: original_length,
            })?;
        self.read_data_at_offset(absolute_start, length)
    }

    /// Build subtype from partition name
    pub fn build_subtype_by_filename(&self, partition_name: &str) -> String {
        let suffix = format!(
            "{}{}",
            partition_name.to_uppercase().replace('.', "_"),
            PARTITION_DOWNLOADFILE_SUFFIX
        );
        let mut subtype = String::with_capacity(16);
        for ch in suffix.chars() {
            if subtype.len() + ch.len_utf8() > 16 {
                break;
            }
            subtype.push(ch);
        }
        while subtype.len() < 16 {
            subtype.push('0');
        }
        subtype
    }

    /// Get image data by predefined name
    pub fn get_image_data_by_name(&mut self, name: &str) -> Result<Vec<u8>, PackerError> {
        if let Some(entry) = crate::firmware::image_data::get_image_data_entry(name) {
            self.get_file_data_by_maintype_subtype(entry.maintype, entry.subtype)
        } else {
            Err(PackerError::FileNotFound(name.to_string()))
        }
    }

    pub fn get_sys_partition(&mut self) -> Result<Vec<u8>, PackerError> {
        self.get_image_data_by_name("sys_partition")
    }

    /// Get FES (Flash Eraser Script) data
    pub fn get_fes(&mut self) -> Result<Vec<u8>, PackerError> {
        self.get_image_data_by_name("fes")
    }

    /// Get U-Boot data
    pub fn get_uboot(&mut self) -> Result<Vec<u8>, PackerError> {
        self.get_image_data_by_name("uboot")
    }

    /// Get MBR (Master Boot Record) data
    pub fn get_mbr(&mut self) -> Result<Vec<u8>, PackerError> {
        self.get_image_data_by_name("mbr")
    }

    /// Get DTB (Device Tree Blob) data
    pub fn get_dtb(&mut self) -> Result<Vec<u8>, PackerError> {
        self.get_image_data_by_name("dtb")
    }

    /// Get system configuration binary data
    pub fn get_sys_config_bin(&mut self) -> Result<Vec<u8>, PackerError> {
        self.get_image_data_by_name("sys_config_bin")
    }

    /// Get board configuration data
    pub fn get_board_config(&mut self) -> Result<Vec<u8>, PackerError> {
        self.get_image_data_by_name("board_config")
    }
}

impl Default for OpenixPacker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{mbr_bytes, temp_file, test_firmware, FirmwareEntry};
    use std::fs;

    fn assert_not_loaded(error: PackerError) {
        assert!(matches!(error, PackerError::ImageNotLoaded));
    }

    #[test]
    fn default_packer_is_empty_and_data_access_reports_not_loaded() {
        let mut packer = OpenixPacker::default();
        assert!(!packer.is_image_loaded());
        assert!(!packer.is_encrypted());
        let info = packer.get_image_info();
        assert_eq!(info.image_size, 0);
        assert_eq!(info.num_files, 0);
        assert!(info.files.is_empty());
        assert!(packer.get_file_header_by_filename("missing").is_none());
        assert!(packer
            .get_file_header_by_maintype_subtype("TYPE", "SUBTYPE")
            .is_none());
        assert!(packer.find_file_header_by_subtype("SUBTYPE").is_none());
        assert_not_loaded(packer.find_file_data_by_subtype("SUBTYPE").unwrap_err());
        assert_not_loaded(packer.get_file_data_by_filename("missing").unwrap_err());
        assert_not_loaded(
            packer
                .get_file_data_by_maintype_subtype("TYPE", "SUBTYPE")
                .unwrap_err(),
        );
        assert!(packer
            .get_file_info_by_maintype_subtype("TYPE", "SUBTYPE")
            .is_none());
        assert!(packer.get_file_info_by_filename("missing").is_none());
        assert_not_loaded(
            packer
                .get_file_data_range_by_maintype_subtype("TYPE", "SUBTYPE", 0, 0)
                .unwrap_err(),
        );
        assert!(matches!(
            packer.get_image_data_by_name("unknown").unwrap_err(),
            PackerError::FileNotFound(_)
        ));
    }

    #[test]
    fn loads_and_reads_entries_through_every_lookup_path() {
        let mbr = mbr_bytes(&[("boot", 0x8000, 0x1000, false)]);
        let firmware = test_firmware(&[
            FirmwareEntry {
                filename: "fes.fex",
                maintype: "FES",
                subtype: "FES_1-0000000000",
                data: b"fes",
            },
            FirmwareEntry {
                filename: "u-boot.fex",
                maintype: "12345678",
                subtype: "UBOOT_0000000000",
                data: b"uboot",
            },
            FirmwareEntry {
                filename: "sunxi_mbr.fex",
                maintype: "12345678",
                subtype: "1234567890___MBR",
                data: &mbr,
            },
            FirmwareEntry {
                filename: "sys_partition.fex",
                maintype: "COMMON",
                subtype: "SYS_CONFIG000000",
                data: b"partition",
            },
            FirmwareEntry {
                filename: "sys_config.bin",
                maintype: "COMMON",
                subtype: "SYS_CONFIG_BIN00",
                data: b"sysconfig",
            },
            FirmwareEntry {
                filename: "board.bin",
                maintype: "COMMON",
                subtype: "BOARD_CONFIG_BIN",
                data: b"board",
            },
            FirmwareEntry {
                filename: "board.dtb",
                maintype: "COMMON",
                subtype: "DTB_CONFIG000000",
                data: b"dtb",
            },
        ]);

        let mut packer = OpenixPacker::new();
        let firmware_path = firmware.path().to_path_buf();
        packer.load(firmware_path).unwrap();
        assert!(packer.is_image_loaded());
        assert!(!packer.is_encrypted());

        let info = packer.get_image_info();
        assert_eq!(info.num_files, 7);
        assert_eq!(info.files.len(), 7);
        assert_eq!(info.header.pid(), 0x1234);
        assert_eq!(info.files[0].filename, "fes.fex");

        assert_eq!(
            packer
                .get_file_header_by_filename("fes.fex")
                .unwrap()
                .subtype_str(),
            "FES_1-0000000000"
        );
        assert!(packer.get_file_header_by_filename("missing").is_none());
        assert!(packer
            .get_file_header_by_maintype_subtype("FES", "FES_1-0000000000")
            .is_some());
        assert!(packer
            .find_file_header_by_subtype("FES_1-0000000000")
            .is_some());

        assert_eq!(
            packer
                .find_file_data_by_subtype("FES_1-0000000000")
                .unwrap(),
            b"fes"
        );
        assert_eq!(
            packer.get_file_data_by_filename("u-boot.fex").unwrap(),
            b"uboot"
        );
        assert_eq!(
            packer
                .get_file_data_by_maintype_subtype("COMMON", "BOARD_CONFIG_BIN")
                .unwrap(),
            b"board"
        );
        assert!(matches!(
            packer.find_file_data_by_subtype("missing").unwrap_err(),
            PackerError::FileNotFound(_)
        ));
        assert!(matches!(
            packer.get_file_data_by_filename("missing").unwrap_err(),
            PackerError::FileNotFound(_)
        ));
        assert!(matches!(
            packer
                .get_file_data_by_maintype_subtype("COMMON", "missing")
                .unwrap_err(),
            PackerError::FileNotFound(_)
        ));

        let (offset, length) = packer
            .get_file_info_by_maintype_subtype("FES", "FES_1-0000000000")
            .unwrap();
        assert!(offset > 0);
        assert_eq!(length, 3);
        assert_eq!(
            packer.get_file_info_by_filename("fes.fex"),
            Some((offset, 3))
        );
        assert_eq!(
            packer
                .get_file_data_range_by_maintype_subtype("FES", "FES_1-0000000000", 1, 2)
                .unwrap(),
            b"es"
        );
        assert!(packer
            .get_file_data_range_by_maintype_subtype("FES", "FES_1-0000000000", 3, 0)
            .unwrap()
            .is_empty());

        assert_eq!(packer.get_fes().unwrap(), b"fes");
        assert_eq!(packer.get_uboot().unwrap(), b"uboot");
        assert_eq!(packer.get_mbr().unwrap(), mbr);
        assert_eq!(packer.get_sys_partition().unwrap(), b"partition");
        assert_eq!(packer.get_sys_config_bin().unwrap(), b"sysconfig");
        assert_eq!(packer.get_board_config().unwrap(), b"board");
        assert_eq!(packer.get_dtb().unwrap(), b"dtb");
        assert!(matches!(
            packer.get_image_data_by_name("unknown").unwrap_err(),
            PackerError::FileNotFound(_)
        ));
    }

    #[test]
    fn load_rejects_missing_truncated_encrypted_unknown_and_forged_headers() {
        let missing = std::env::temp_dir().join(format!(
            "openixcli-definitely-missing-{}.bin",
            std::process::id()
        ));
        let _ = fs::remove_file(&missing);
        assert!(matches!(
            OpenixPacker::new().load(&missing),
            Err(PackerError::Io(_))
        ));

        let short = temp_file("short", b"IMAGE");
        assert!(matches!(
            OpenixPacker::new().load(short.path()),
            Err(PackerError::Io(_))
        ));

        let encrypted = temp_file("encrypted", b"NOTIMAGE");
        let mut packer = OpenixPacker::new();
        assert!(matches!(
            packer.load(encrypted.path()),
            Err(PackerError::EncryptedNotSupported)
        ));
        assert!(packer.is_encrypted());
        assert!(!packer.is_image_loaded());

        let valid = test_firmware(&[]);
        let mut unknown_bytes = fs::read(valid.path()).unwrap();
        unknown_bytes[8..12].copy_from_slice(&0x9999u32.to_le_bytes());
        let unknown = temp_file("unknown-version", &unknown_bytes);
        assert!(matches!(
            OpenixPacker::new().load(unknown.path()),
            Err(PackerError::UnknownHeaderVersion(0x9999))
        ));

        let mut forged_bytes = fs::read(valid.path()).unwrap();
        forged_bytes[56..60].copy_from_slice(&u32::MAX.to_le_bytes());
        let forged = temp_file("forged-count", &forged_bytes);
        assert!(matches!(
            OpenixPacker::new().load(forged.path()),
            Err(PackerError::ParseError(
                "File header table exceeds firmware size"
            ))
        ));

        let working = test_firmware(&[FirmwareEntry {
            filename: "ok.bin",
            maintype: "COMMON",
            subtype: "OK00000000000000",
            data: b"ok",
        }]);
        packer.load(working.path()).unwrap();
        assert!(packer.is_image_loaded());
        assert!(packer.load(encrypted.path()).is_err());
        assert!(!packer.is_image_loaded());
        assert!(packer.get_image_info().files.is_empty());
    }

    #[test]
    fn range_reads_reject_out_of_bounds_overflow_and_truncated_payloads() {
        assert!(matches!(
            OpenixPacker::new().read_data_at_offset(0, 0),
            Err(PackerError::ImageNotLoaded)
        ));

        let firmware = test_firmware(&[FirmwareEntry {
            filename: "data.bin",
            maintype: "COMMON",
            subtype: "DATA000000000000",
            data: b"abc",
        }]);
        let mut packer = OpenixPacker::new();
        packer.load(firmware.path()).unwrap();

        assert!(matches!(
            packer.read_data_at_offset(u64::MAX, 1),
            Err(PackerError::InvalidRange { .. })
        ));

        for (start, length) in [(2, 2), (u64::MAX, 1)] {
            assert!(matches!(
                packer.get_file_data_range_by_maintype_subtype(
                    "COMMON",
                    "DATA000000000000",
                    start,
                    length
                ),
                Err(PackerError::InvalidRange { .. })
            ));
        }

        let mut truncated_bytes = fs::read(firmware.path()).unwrap();
        truncated_bytes.truncate(truncated_bytes.len() - 1);
        let truncated = temp_file("truncated-payload", &truncated_bytes);
        let mut truncated_packer = OpenixPacker::new();
        truncated_packer.load(truncated.path()).unwrap();
        assert!(matches!(
            truncated_packer.get_file_data_by_filename("data.bin"),
            Err(PackerError::InvalidRange { .. })
        ));
    }

    #[test]
    fn subtype_builder_is_fixed_width_and_utf8_safe() {
        let packer = OpenixPacker::new();
        assert_eq!(
            packer.build_subtype_by_filename("boot.fex"),
            "BOOT_FEX00000000"
        );
        assert_eq!(
            packer.build_subtype_by_filename("abcdefghijklmnopq"),
            "ABCDEFGHIJKLMNOP"
        );
        let unicode = packer.build_subtype_by_filename("系统.img");
        assert_eq!(unicode.len(), 16);
        assert!(unicode.starts_with("系统_IMG"));
    }

    #[test]
    fn predefined_getters_report_missing_entries() {
        let firmware = test_firmware(&[]);
        let mut packer = OpenixPacker::new();
        packer.load(firmware.path()).unwrap();
        assert!(matches!(
            packer.get_sys_partition(),
            Err(PackerError::FileNotFound(_))
        ));
        assert!(matches!(
            packer.get_fes(),
            Err(PackerError::FileNotFound(_))
        ));
        assert!(matches!(
            packer.get_uboot(),
            Err(PackerError::FileNotFound(_))
        ));
        assert!(matches!(
            packer.get_mbr(),
            Err(PackerError::FileNotFound(_))
        ));
        assert!(matches!(
            packer.get_dtb(),
            Err(PackerError::FileNotFound(_))
        ));
        assert!(matches!(
            packer.get_sys_config_bin(),
            Err(PackerError::FileNotFound(_))
        ));
        assert!(matches!(
            packer.get_board_config(),
            Err(PackerError::FileNotFound(_))
        ));
    }
}
