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
        maintype: "BOOTPKG",
        subtype: "BOOTPKG-00000000",
    },
    ImageDataEntry {
        name: "bootpkg_nor",
        maintype: "BOOTPKG",
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

pub fn get_maintype_subtype(name: &str) -> Option<(&'static str, &'static str)> {
    IMAGE_ENTRY_MAP
        .get(name)
        .map(|entry| (entry.maintype, entry.subtype))
}
