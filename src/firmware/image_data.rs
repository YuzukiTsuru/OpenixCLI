#![allow(dead_code)]

pub struct ImageDataEntry {
    pub name: &'static str,
    pub maintype: &'static str,
    pub subtype: &'static str,
}

pub const IMAGE_DATA_TABLE: &[ImageDataEntry] = &[
    ImageDataEntry {
        name: "fes",
        maintype: "FES",
        subtype: "FES_1-0000000000",
    },
    ImageDataEntry {
        name: "uboot",
        maintype: "12345678",
        subtype: "UBOOT_0000000000",
    },
    ImageDataEntry {
        name: "uboot_crash",
        maintype: "12345678",
        subtype: "UBOOT_CRASH_0000",
    },
    ImageDataEntry {
        name: "mbr",
        maintype: "12345678",
        subtype: "1234567890___MBR",
    },
    ImageDataEntry {
        name: "gpt",
        maintype: "12345678",
        subtype: "1234567890___GPT",
    },
    ImageDataEntry {
        name: "sys_config",
        maintype: "COMMON",
        subtype: "SYS_CONFIG100000",
    },
    ImageDataEntry {
        name: "sys_config_bin",
        maintype: "COMMON",
        subtype: "SYS_CONFIG_BIN00",
    },
    ImageDataEntry {
        name: "sys_partition",
        maintype: "COMMON",
        subtype: "SYS_CONFIG000000",
    },
    ImageDataEntry {
        name: "board_config",
        maintype: "COMMON",
        subtype: "BOARD_CONFIG_BIN",
    },
    ImageDataEntry {
        name: "dtb",
        maintype: "COMMON",
        subtype: "DTB_CONFIG000000",
    },
    ImageDataEntry {
        name: "boot0_nand",
        maintype: "BOOT",
        subtype: "BOOT0_0000000000",
    },
    ImageDataEntry {
        name: "boot0_card",
        maintype: "12345678",
        subtype: "1234567890BOOT_0",
    },
    ImageDataEntry {
        name: "boot0_nor",
        maintype: "12345678",
        subtype: "1234567890BNOR_0",
    },
    ImageDataEntry {
        name: "bootpkg",
        maintype: "12345678",
        subtype: "BOOTPKG-00000000",
    },
    ImageDataEntry {
        name: "bootpkg_nor",
        maintype: "12345678",
        subtype: "BOOTPKG-NOR00000",
    },
];

use once_cell::sync::Lazy;
use std::collections::HashMap;

static IMAGE_ENTRY_MAP: Lazy<HashMap<&'static str, &'static ImageDataEntry>> = Lazy::new(|| {
    IMAGE_DATA_TABLE
        .iter()
        .map(|entry| (entry.name, entry))
        .collect()
});

pub fn get_image_data_entry(name: &str) -> Option<&'static ImageDataEntry> {
    IMAGE_ENTRY_MAP.get(name).copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::image_cfg_entries;

    #[test]
    fn every_declared_image_entry_is_lookupable() {
        for expected in IMAGE_DATA_TABLE {
            let actual = get_image_data_entry(expected.name).unwrap();
            assert_eq!(actual.name, expected.name);
            assert_eq!(actual.maintype, expected.maintype);
            assert_eq!(actual.subtype, expected.subtype);
        }
        assert!(get_image_data_entry("unknown").is_none());
        assert!(get_image_data_entry("").is_none());
    }

    #[test]
    fn predefined_entries_match_the_real_image_cfg_filelist() {
        let config_entries = image_cfg_entries();
        assert_eq!(config_entries.len(), 28);

        let mappings = [
            ("fes", "fes1.fex"),
            ("uboot", "u-boot-efex.fex"),
            ("uboot_crash", "u-boot-crash.fex"),
            ("mbr", "sunxi_mbr.fex"),
            ("gpt", "sunxi_gpt.fex"),
            ("sys_config", "sys_config.fex"),
            ("sys_config_bin", "config.fex"),
            ("sys_partition", "sys_partition.fex"),
            ("board_config", "board.fex"),
            ("dtb", "sunxi.fex"),
            ("boot0_nand", "boot0_nand.fex"),
            ("boot0_card", "boot0_sdcard.fex"),
            ("bootpkg", "boot_package.fex"),
        ];

        for (name, filename) in mappings {
            let expected = config_entries
                .iter()
                .find(|entry| entry.filename == filename)
                .unwrap_or_else(|| panic!("{filename} missing from image.cfg"));
            let actual = get_image_data_entry(name).unwrap();
            assert_eq!(actual.maintype, expected.maintype, "{filename} maintype");
            assert_eq!(actual.subtype, expected.subtype, "{filename} subtype");
        }
    }
}
