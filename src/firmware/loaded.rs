//! Firmware loading helper used by CLI and TUI.

use std::path::Path;

use crate::config::mbr_parser::SunxiMbr;

use super::{ImageInfo, OpenixPacker, PackerError};

/// Loaded firmware with metadata needed by frontends.
pub struct LoadedFirmware {
    packer: OpenixPacker,
    image_info: ImageInfo,
    partition_names: Vec<String>,
}

impl LoadedFirmware {
    pub fn load(path: &Path) -> Result<Self, PackerError> {
        let mut packer = OpenixPacker::new();
        packer.load(path)?;

        let image_info = packer.get_image_info();
        let partition_names = match packer.get_mbr() {
            Ok(mbr_data) => match SunxiMbr::parse(&mbr_data) {
                Ok(mbr) => mbr.partitions.iter().map(|p| p.name.clone()).collect(),
                Err(_) => Vec::new(),
            },
            Err(_) => Vec::new(),
        };

        Ok(Self {
            packer,
            image_info,
            partition_names,
        })
    }

    pub fn image_info(&self) -> &ImageInfo {
        &self.image_info
    }

    pub fn partition_names(&self) -> &[String] {
        &self.partition_names
    }

    pub fn into_packer(self) -> OpenixPacker {
        self.packer
    }
}
