#![allow(dead_code)]

use crate::firmware::types::*;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const PARTITION_DOWNLOADFILE_SUFFIX: &str = "0000000000";

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
}

pub struct OpenixPacker {
    file: Option<File>,
    image_header: Option<ImageHeader>,
    file_headers: Vec<FileHeader>,
    is_encrypted: bool,
    image_loaded: bool,
}

impl OpenixPacker {
    pub fn new() -> Self {
        Self {
            file: None,
            image_header: None,
            file_headers: Vec::new(),
            is_encrypted: false,
            image_loaded: false,
        }
    }

    pub fn load<P: AsRef<Path>>(&mut self, path: P) -> Result<(), PackerError> {
        let mut file = File::open(path)?;

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
        let num_files = image_header.num_files();

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

    pub fn is_image_loaded(&self) -> bool {
        self.image_loaded
    }

    pub fn is_encrypted(&self) -> bool {
        self.is_encrypted
    }

    pub fn get_image_info(&self) -> ImageInfo {
        let header = match self.image_header {
            Some(ref h) => *h,
            None => ImageHeader {
                magic: [0u8; IMAGEWTY_MAGIC_LEN],
                header_version: 0,
                header_size: 0,
                ram_base: 0,
                version: 0,
                image_size: 0,
                image_header_size: 0,
                data: ImageHeaderVersionData {
                    v1: ImageHeaderV1 {
                        pid: 0,
                        vid: 0,
                        hardware_id: 0,
                        firmware_id: 0,
                        val1: 0,
                        val1024: 0,
                        num_files: 0,
                        val1024_2: 0,
                        val0: 0,
                        val0_2: 0,
                        val0_3: 0,
                        val0_4: 0,
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
            image_size: header.image_size,
            num_files: header.num_files(),
            header,
            files,
            is_encrypted: self.is_encrypted,
        }
    }

    fn get_header_version(&self) -> u32 {
        self.image_header
            .as_ref()
            .map(|h| h.header_version)
            .unwrap_or(0)
    }

    pub fn get_file_header_by_filename(&self, filename: &str) -> Option<&FileHeader> {
        let header_version = self.get_header_version();
        self.file_headers
            .iter()
            .find(|fh| fh.filename_str(header_version) == filename)
    }

    pub fn get_file_header_by_maintype_subtype(
        &self,
        maintype: &str,
        subtype: &str,
    ) -> Option<&FileHeader> {
        self.file_headers
            .iter()
            .find(|fh| fh.maintype_str() == maintype && fh.subtype_str() == subtype)
    }

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
            file_header.offset(header_version) as u64,
            file_header.original_length(header_version) as u64,
        ))
    }

    pub fn get_file_info_by_filename(&self, filename: &str) -> Option<(u64, u64)> {
        if !self.image_loaded {
            return None;
        }

        let header_version = self.get_header_version();
        let file_header = self.get_file_header_by_filename(filename)?;
        Some((
            file_header.offset(header_version) as u64,
            file_header.original_length(header_version) as u64,
        ))
    }

    fn read_data_at_offset(&mut self, offset: u32, length: u32) -> Result<Vec<u8>, PackerError> {
        let file = self.file.as_mut().ok_or(PackerError::ImageNotLoaded)?;

        file.seek(SeekFrom::Start(offset as u64))?;

        let mut buffer = vec![0u8; length as usize];
        file.read_exact(&mut buffer)?;

        Ok(buffer)
    }

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

        let original_length = file_header.original_length(header_version) as u64;
        if start + length > original_length {
            return Err(PackerError::FileNotFound(format!(
                "Range out of bounds: {} + {} > {}",
                start, length, original_length
            )));
        }

        self.read_data_at_offset(
            (file_header.offset(header_version) as u64 + start) as u32,
            length as u32,
        )
    }

    pub fn build_subtype_by_filename(&self, partition_name: &str) -> String {
        let suffix = format!(
            "{}{}",
            partition_name.to_uppercase().replace('.', "_"),
            PARTITION_DOWNLOADFILE_SUFFIX
        );
        if suffix.len() >= 16 {
            suffix[..16].to_string()
        } else {
            format!("{:0<16}", suffix)
        }
    }

    pub fn get_image_data_by_name(&mut self, name: &str) -> Result<Vec<u8>, PackerError> {
        if let Some(entry) = crate::firmware::image_data::get_image_data_entry(name) {
            self.get_file_data_by_maintype_subtype(entry.maintype, entry.subtype)
        } else {
            Err(PackerError::FileNotFound(name.to_string()))
        }
    }

    pub fn get_fes(&mut self) -> Result<Vec<u8>, PackerError> {
        self.get_image_data_by_name("fes")
    }

    pub fn get_uboot(&mut self) -> Result<Vec<u8>, PackerError> {
        self.get_image_data_by_name("uboot")
    }

    pub fn get_mbr(&mut self) -> Result<Vec<u8>, PackerError> {
        self.get_image_data_by_name("mbr")
    }

    pub fn get_dtb(&mut self) -> Result<Vec<u8>, PackerError> {
        self.get_image_data_by_name("dtb")
    }

    pub fn get_sys_config_bin(&mut self) -> Result<Vec<u8>, PackerError> {
        self.get_image_data_by_name("sys_config_bin")
    }

    pub fn get_board_config(&mut self) -> Result<Vec<u8>, PackerError> {
        self.get_image_data_by_name("board_config")
    }
}

impl Default for OpenixPacker {
    fn default() -> Self {
        Self::new()
    }
}
