//! Conversion from Allwinner IMAGEWTY packages to raw programmer images.

mod block;
mod flashmap;
mod gpt;
mod spinor;

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use clap::ValueEnum;
use log::{info, warn};

use crate::config::partition::OpenixPartition;
use crate::firmware::OpenixPacker;

use flashmap::{extract_dtb_from_uboot, parse_flash_offsets, FlashOffsets};

const SECTOR_SIZE: u64 = 512;
const MEBIBYTE: u64 = 1024 * 1024;

/// Target storage layout for raw programmer image conversion.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum ConvertTarget {
    /// SPI NOR full-flash image.
    Spinor,
    /// SD card programmer image.
    Sdcard,
    /// eMMC programmer image.
    Emmc,
    /// SD NAND programmer image (uses the eMMC boot layout).
    Sdnand,
    /// UFS programmer image.
    Ufs,
}

impl ConvertTarget {
    fn output_suffix(self) -> &'static str {
        match self {
            Self::Spinor => "_full_img.bin",
            _ => "_programmer.bin",
        }
    }

    fn flash_type(self) -> &'static str {
        match self {
            Self::Ufs => "ufs",
            Self::Sdnand => "emmc",
            Self::Sdcard => "sdcard",
            Self::Emmc => "emmc",
            Self::Spinor => "spinor",
        }
    }
}

/// Controls secure boot component selection during conversion.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum SecureMode {
    /// Detect a non-placeholder TOC1 entry in the input firmware.
    Auto,
    /// Force TOC0/TOC1 secure boot components.
    Enabled,
    /// Force Boot0/BOOTPKG normal boot components.
    Disabled,
}

/// User-facing conversion options.
#[derive(Debug, Clone)]
pub struct ConvertOptions {
    pub firmware_path: PathBuf,
    pub output: Option<PathBuf>,
    pub target: ConvertTarget,
    /// Logical offset in 512-byte sectors.
    pub logic_offset: Option<u64>,
    /// SPI NOR U-Boot offset in 512-byte sectors.
    pub uboot_start: Option<u64>,
    /// SPI NOR capacity in MiB.
    pub nor_size: u64,
    /// GPT target size (`auto`, a byte count, or a KB/MB/GB value).
    pub storage_size: String,
    pub secure: SecureMode,
}

/// Successful conversion summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConvertResult {
    pub output_path: PathBuf,
    pub output_size: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct PartitionEntry {
    pub(crate) name: String,
    pub(crate) size: u64,
    pub(crate) download_file: String,
}

pub(crate) struct SpinorConfig {
    pub(crate) output_path: PathBuf,
    pub(crate) logic_start: u64,
    pub(crate) uboot_start: u64,
    pub(crate) partitions: Vec<PartitionEntry>,
    pub(crate) nor_size: u64,
}

pub(crate) struct BlockConfig {
    pub(crate) output_path: PathBuf,
    pub(crate) logic_offset: u64,
    pub(crate) partitions: Vec<PartitionEntry>,
    pub(crate) flash_type: &'static str,
    pub(crate) is_secure: bool,
    pub(crate) storage_size: Option<u64>,
}

pub(crate) fn default_output_path(input: &Path, target: ConvertTarget) -> PathBuf {
    let stem = input.file_stem().unwrap_or(input.as_os_str());
    let mut name = OsString::from(stem);
    name.push(target.output_suffix());
    input.with_file_name(name)
}

pub(crate) fn parse_storage_size(value: &str) -> Result<Option<u64>, String> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized.is_empty() || normalized == "auto" {
        return Ok(None);
    }

    let (number, multiplier) = if let Some(number) = normalized.strip_suffix("gb") {
        (number, 1024_u64.pow(3))
    } else if let Some(number) = normalized.strip_suffix("mb") {
        (number, 1024_u64.pow(2))
    } else if let Some(number) = normalized.strip_suffix("kb") {
        (number, 1024_u64)
    } else if let Some(number) = normalized.strip_suffix('g') {
        (number, 1024_u64.pow(3))
    } else if let Some(number) = normalized.strip_suffix('m') {
        (number, 1024_u64.pow(2))
    } else if let Some(number) = normalized.strip_suffix('k') {
        (number, 1024_u64)
    } else {
        (normalized.as_str(), 1)
    };

    let amount = number
        .trim()
        .parse::<u64>()
        .map_err(|_| format!("Invalid storage size: {value}"))?;
    amount
        .checked_mul(multiplier)
        .map(Some)
        .ok_or_else(|| format!("Storage size overflows u64: {value}"))
}

fn checked_scale(value: u64, scale: u64, label: &str) -> Result<u64, String> {
    value
        .checked_mul(scale)
        .ok_or_else(|| format!("{label} overflows u64"))
}

fn load_partitions(packer: &mut OpenixPacker) -> Result<Vec<PartitionEntry>, String> {
    let data = match packer.get_sys_partition() {
        Ok(data) => data,
        Err(_) => {
            warn!("sys_partition was not found; converting boot components only");
            return Ok(Vec::new());
        }
    };
    let mut parser = OpenixPartition::new();
    parser.parse_from_data(&data);
    parser
        .get_partitions()
        .iter()
        .map(|partition| {
            Ok(PartitionEntry {
                name: partition.name.clone(),
                size: checked_scale(
                    partition.size,
                    SECTOR_SIZE,
                    &format!("Partition {} size", partition.name),
                )?,
                download_file: partition.downloadfile.clone(),
            })
        })
        .collect()
}

fn detect_secure_firmware(packer: &OpenixPacker) -> bool {
    packer
        .get_image_info()
        .files
        .iter()
        .any(|file| file.subtype == "TOC1_00000000000" && file.original_length > 8)
}

fn detect_flash_offsets(packer: &mut OpenixPacker) -> FlashOffsets {
    if let Ok(dtb) = packer.get_dtb() {
        if let Ok(offsets) = parse_flash_offsets(&dtb) {
            return offsets;
        }
    }
    if let Ok(uboot) = packer.get_uboot() {
        if let Some(dtb) = extract_dtb_from_uboot(&uboot) {
            if let Ok(offsets) = parse_flash_offsets(dtb) {
                return offsets;
            }
        }
    }
    FlashOffsets::default()
}

fn reject_input_as_output(input: &Path, output: &Path) -> Result<(), String> {
    if input == output {
        return Err("Output path must differ from the input firmware path".to_string());
    }
    if output.exists() {
        let input = input
            .canonicalize()
            .map_err(|error| format!("Failed to resolve input path: {error}"))?;
        let output = output
            .canonicalize()
            .map_err(|error| format!("Failed to resolve output path: {error}"))?;
        if input == output {
            return Err("Output path must differ from the input firmware path".to_string());
        }
    }
    Ok(())
}

/// Convert one IMAGEWTY firmware package into a raw programmer image.
pub fn convert(options: ConvertOptions) -> Result<ConvertResult, String> {
    if !options.firmware_path.is_file() {
        return Err(format!(
            "Firmware file not found: {}",
            options.firmware_path.display()
        ));
    }
    let output_path = options
        .output
        .clone()
        .unwrap_or_else(|| default_output_path(&options.firmware_path, options.target));
    reject_input_as_output(&options.firmware_path, &output_path)?;

    let mut packer = OpenixPacker::new();
    packer
        .load(&options.firmware_path)
        .map_err(|error| format!("Failed to load firmware: {error}"))?;

    let partitions = load_partitions(&mut packer)?;
    let offsets = detect_flash_offsets(&mut packer);
    let detected_secure = detect_secure_firmware(&packer);
    let is_secure = match options.secure {
        SecureMode::Auto => detected_secure,
        SecureMode::Enabled => true,
        SecureMode::Disabled => false,
    };

    let output_size = if options.target == ConvertTarget::Spinor {
        let logic_start = options
            .logic_offset
            .or(offsets.nor_logic_offset)
            .unwrap_or(1024);
        let uboot_start = options
            .uboot_start
            .or(offsets.nor_uboot_start)
            .unwrap_or(48);
        let config = SpinorConfig {
            output_path: output_path.clone(),
            logic_start: checked_scale(logic_start, SECTOR_SIZE, "Logic offset")?,
            uboot_start: checked_scale(uboot_start, SECTOR_SIZE, "U-Boot offset")?,
            partitions,
            nor_size: checked_scale(options.nor_size, MEBIBYTE, "NOR size")?,
        };
        info!(
            "Converting SPI NOR image: logic_offset={} sectors, uboot_start={} sectors",
            logic_start, uboot_start
        );
        spinor::merge(&mut packer, config)?
    } else {
        let logic_offset = options
            .logic_offset
            .or(offsets.sdmmc_logic_offset)
            .unwrap_or(40960);
        let config = BlockConfig {
            output_path: output_path.clone(),
            logic_offset: checked_scale(logic_offset, SECTOR_SIZE, "Logic offset")?,
            partitions,
            flash_type: options.target.flash_type(),
            is_secure,
            storage_size: parse_storage_size(&options.storage_size)?,
        };
        info!(
            "Converting {} image: logic_offset={} sectors, secure={}",
            config.flash_type, logic_offset, is_secure
        );
        block::merge(&mut packer, config)?
    };

    Ok(ConvertResult {
        output_path,
        output_size,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{mbr_bytes, temp_dir, test_firmware, FirmwareEntry};
    use std::fs;

    #[test]
    fn default_output_names_match_the_firmware_packer_workflow() {
        assert_eq!(
            default_output_path(Path::new("firmware.img"), ConvertTarget::Emmc),
            Path::new("firmware_programmer.bin")
        );
        assert_eq!(
            default_output_path(Path::new("firmware.img"), ConvertTarget::Spinor),
            Path::new("firmware_full_img.bin")
        );
        assert_eq!(
            default_output_path(Path::new("firmware"), ConvertTarget::Ufs),
            Path::new("firmware_programmer.bin")
        );
    }

    #[test]
    fn storage_sizes_accept_packer_units_and_reject_invalid_values() {
        assert_eq!(parse_storage_size("auto").unwrap(), None);
        assert_eq!(
            parse_storage_size("8GB").unwrap(),
            Some(8 * 1024 * 1024 * 1024)
        );
        assert_eq!(
            parse_storage_size("512 mb").unwrap(),
            Some(512 * 1024 * 1024)
        );
        assert!(parse_storage_size("1.5GB").is_err());
        assert!(parse_storage_size("18446744073709551615GB").is_err());
    }

    #[test]
    fn converts_block_firmware_using_partition_config_and_mbr() {
        let subtype = OpenixPacker::new().build_subtype_by_filename("rootfs.img");
        let partition_config =
            b"[partition_start]\n[partition]\nname=rootfs\nsize=8\ndownloadfile=rootfs.img\n";
        let mbr = mbr_bytes(&[("rootfs", 0, 8, false)]);
        let firmware = test_firmware(&[
            FirmwareEntry {
                filename: "boot0.fex",
                maintype: "12345678",
                subtype: "1234567890BOOT_0",
                data: &[0xb0, 0xb1],
            },
            FirmwareEntry {
                filename: "sys_partition.fex",
                maintype: "COMMON",
                subtype: "SYS_CONFIG000000",
                data: partition_config,
            },
            FirmwareEntry {
                filename: "sunxi_mbr.fex",
                maintype: "12345678",
                subtype: "1234567890___MBR",
                data: &mbr,
            },
            FirmwareEntry {
                filename: "rootfs.img",
                maintype: "RFSFAT16",
                subtype: &subtype,
                data: &[1, 2, 3, 4],
            },
        ]);
        let directory = temp_dir("convert-block");
        let output = directory.path().join("programmer.img");

        let result = convert(ConvertOptions {
            firmware_path: firmware.path().to_path_buf(),
            output: Some(output.clone()),
            target: ConvertTarget::Emmc,
            logic_offset: Some(64),
            uboot_start: None,
            nor_size: 16,
            storage_size: "auto".to_string(),
            secure: SecureMode::Auto,
        })
        .unwrap();

        let bytes = fs::read(&output).unwrap();
        let partition_offset = 64 * SECTOR_SIZE + 64 * 1024;
        assert_eq!(result.output_path, output);
        assert_eq!(result.output_size, bytes.len() as u64);
        assert_eq!(
            &bytes[BOOT_TEST_OFFSET..BOOT_TEST_OFFSET + 2],
            &[0xb0, 0xb1]
        );
        assert_eq!(
            &bytes[partition_offset as usize..partition_offset as usize + 4],
            &[1, 2, 3, 4]
        );
    }

    #[test]
    fn converts_spinor_firmware_and_preserves_erased_regions() {
        let subtype = OpenixPacker::new().build_subtype_by_filename("rootfs.img");
        let partition_config =
            b"[partition_start]\n[partition]\nname=rootfs\nsize=8\ndownloadfile=rootfs.img\n";
        let firmware = test_firmware(&[
            FirmwareEntry {
                filename: "boot0.fex",
                maintype: "12345678",
                subtype: "1234567890BNOR_0",
                data: &[0xb0, 0xb1],
            },
            FirmwareEntry {
                filename: "boot_package.fex",
                maintype: "12345678",
                subtype: "BOOTPKG-NOR00000",
                data: &[0xa0, 0xa1],
            },
            FirmwareEntry {
                filename: "sys_partition.fex",
                maintype: "COMMON",
                subtype: "SYS_CONFIG000000",
                data: partition_config,
            },
            FirmwareEntry {
                filename: "sunxi_gpt.fex",
                maintype: "12345678",
                subtype: "1234567890___GPT",
                data: &[0x47, 0x50, 0x54],
            },
            FirmwareEntry {
                filename: "rootfs.img",
                maintype: "RFSFAT16",
                subtype: &subtype,
                data: &[1, 2, 3, 4],
            },
        ]);
        let directory = temp_dir("convert-spinor");
        let output = directory.path().join("spinor.img");

        let result = convert(ConvertOptions {
            firmware_path: firmware.path().to_path_buf(),
            output: Some(output.clone()),
            target: ConvertTarget::Spinor,
            logic_offset: Some(2),
            uboot_start: Some(1),
            nor_size: 1,
            storage_size: "auto".to_string(),
            secure: SecureMode::Auto,
        })
        .unwrap();

        let bytes = fs::read(&output).unwrap();
        let partition_offset = 2 * SECTOR_SIZE + 16 * 1024;
        assert_eq!(result.output_size, 1024 * 1024);
        assert_eq!(&bytes[..2], &[0xb0, 0xb1]);
        assert_eq!(
            &bytes[SECTOR_SIZE as usize..SECTOR_SIZE as usize + 2],
            &[0xa0, 0xa1]
        );
        assert_eq!(
            &bytes[partition_offset as usize..partition_offset as usize + 4],
            &[1, 2, 3, 4]
        );
        assert_eq!(bytes[100], 0xff);
    }

    const BOOT_TEST_OFFSET: usize = 16 * SECTOR_SIZE as usize;
}
