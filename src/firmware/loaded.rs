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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{mbr_bytes, temp_file, test_firmware, FirmwareEntry};

    #[test]
    fn loads_metadata_partitions_and_returns_the_packer() {
        let mbr = mbr_bytes(&[
            ("boot", 0x8000, 0x1000, false),
            ("rootfs", 0x9000, 0x2000, false),
        ]);
        let firmware = test_firmware(&[FirmwareEntry {
            filename: "sunxi_mbr.fex",
            maintype: "12345678",
            subtype: "1234567890___MBR",
            data: &mbr,
        }]);

        let loaded = LoadedFirmware::load(firmware.path()).unwrap();
        assert_eq!(loaded.image_info().num_files, 1);
        assert_eq!(loaded.partition_names(), &["boot", "rootfs"]);
        let mut packer = loaded.into_packer();
        assert_eq!(packer.get_mbr().unwrap(), mbr);
    }

    #[test]
    fn missing_or_invalid_mbr_produces_an_empty_partition_list() {
        let no_mbr = test_firmware(&[]);
        assert!(LoadedFirmware::load(no_mbr.path())
            .unwrap()
            .partition_names()
            .is_empty());

        let invalid_mbr = test_firmware(&[FirmwareEntry {
            filename: "sunxi_mbr.fex",
            maintype: "12345678",
            subtype: "1234567890___MBR",
            data: b"invalid",
        }]);
        assert!(LoadedFirmware::load(invalid_mbr.path())
            .unwrap()
            .partition_names()
            .is_empty());
    }

    #[test]
    fn load_propagates_packer_errors() {
        let invalid = temp_file("loaded-invalid", b"NOTIMAGE");
        assert!(matches!(
            LoadedFirmware::load(invalid.path()),
            Err(PackerError::EncryptedNotSupported)
        ));
    }
}
