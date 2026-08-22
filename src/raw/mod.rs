//! Standalone raw-image flashing.
//!
//! The device boots from a private IMAGEWTY firmware, which is then used to write a standard
//! firmware image directly into the target storage, block by block — analogous to a `dd` operation.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::ValueEnum;

use crate::config::mbr_parser::{MBR_MAGIC, MBR_SIZE, MBR_VERSION};
use crate::firmware::OpenixPacker;
use crate::flash::{
    CustomFlashLayout, DeviceSelector, ExternalPartition, FlashMode, FlashRequest, PostAction,
};

const SECTOR_SIZE: u64 = 512;
const LOGIC_ADDRESS_LIMIT: u64 = 0x1_0000_0000;
const DEFAULT_LOGIC_OFFSET: u64 = 40960;
const DEFAULT_NOR_LOGIC_OFFSET: u64 = 2106;
const PARTITION_OFFSET: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum RawMode {
    Command,
    LogicOffset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum RawStorage {
    Sdmmc,
    Ufs,
    Nor,
}

impl RawStorage {
    pub fn default_logic_offset(self) -> u64 {
        match self {
            Self::Nor => DEFAULT_NOR_LOGIC_OFFSET,
            Self::Sdmmc | Self::Ufs => DEFAULT_LOGIC_OFFSET,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawOptions {
    pub firmware_path: PathBuf,
    pub image_path: PathBuf,
    pub mode: RawMode,
    pub storage: RawStorage,
    pub logic_offset: Option<u64>,
    pub bus: Option<u8>,
    pub port: Option<u8>,
    pub verbose: bool,
}

pub fn prepare(options: &RawOptions) -> Result<(OpenixPacker, FlashRequest), String> {
    if !options.firmware_path.exists() {
        return Err(format!(
            "Firmware file not found: {}",
            options.firmware_path.display()
        ));
    }
    if !options.image_path.exists() {
        return Err(format!(
            "Raw image file not found: {}",
            options.image_path.display()
        ));
    }

    let image_size = std::fs::metadata(&options.image_path)
        .map_err(|error| format!("Failed to inspect raw image: {error}"))?
        .len();
    if image_size == 0 {
        return Err("Raw image is empty".to_string());
    }

    let mut packer = OpenixPacker::new();
    packer
        .load(&options.firmware_path)
        .map_err(|error| format!("Failed to load firmware: {error}"))?;

    let boot_mbr_copy = if options.mode == RawMode::LogicOffset {
        valid_boot_mbr_copy(&mut packer).unwrap_or(1)
    } else {
        1
    };
    let layout = build_layout(
        options.image_path.clone(),
        image_size,
        options.mode,
        options.storage,
        options.logic_offset,
        boot_mbr_copy,
    )?;
    let request = FlashRequest::new(
        DeviceSelector::new(options.bus, options.port),
        false,
        FlashMode::FullErase,
        None,
        PostAction::Reboot,
    )
    .with_custom_layout(layout);

    Ok((packer, request))
}

fn valid_boot_mbr_copy(packer: &mut OpenixPacker) -> Option<u32> {
    let data = packer.get_mbr().ok()?;
    let first_copy = data.get(..MBR_SIZE)?;
    let stored_crc = u32::from_le_bytes(first_copy[..4].try_into().ok()?);
    if stored_crc != crc32fast::hash(&first_copy[4..]) {
        return None;
    }
    crate::config::mbr_parser::SunxiMbr::parse(first_copy)
        .ok()
        .map(|mbr| mbr.copy.max(1))
}

fn image_size_in_sectors(image_size: u64) -> u64 {
    image_size.div_ceil(SECTOR_SIZE)
}

fn build_layout(
    image_path: PathBuf,
    image_size: u64,
    mode: RawMode,
    storage: RawStorage,
    logic_offset: Option<u64>,
    boot_mbr_copy: u32,
) -> Result<CustomFlashLayout, String> {
    let (address, copies) = match mode {
        RawMode::Command => (0, 1),
        RawMode::LogicOffset => {
            let offset = logic_offset.unwrap_or_else(|| storage.default_logic_offset());
            if offset > LOGIC_ADDRESS_LIMIT {
                return Err(format!(
                    "Logic offset must be between 0 and {LOGIC_ADDRESS_LIMIT} sectors"
                ));
            }
            (LOGIC_ADDRESS_LIMIT - offset, boot_mbr_copy.max(1))
        }
    };
    let length_sectors = image_size_in_sectors(image_size);
    let mbr_data = build_virtual_mbr(address, length_sectors, copies)?;
    Ok(CustomFlashLayout::new(
        mbr_data,
        vec![
            ExternalPartition::new("raw", image_path, address, image_size)
                .with_wrapping_address(mode == RawMode::LogicOffset),
        ],
    ))
}

pub(crate) fn build_virtual_mbr(
    address: u64,
    length_sectors: u64,
    copy_count: u32,
) -> Result<Vec<u8>, String> {
    let copies = copy_count.max(1);
    let copies_usize = usize::try_from(copies).map_err(|_| "MBR copy count is too large")?;
    let total_size = MBR_SIZE
        .checked_mul(copies_usize)
        .ok_or("MBR copy data is too large")?;

    let mut single = vec![0u8; MBR_SIZE];
    write_u32(&mut single, 4, MBR_VERSION);
    single[8..16].copy_from_slice(MBR_MAGIC.as_bytes());
    write_u32(&mut single, 16, copies);
    write_u32(&mut single, 24, 1);
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as u32;
    write_u32(&mut single, 28, stamp);

    write_u32(&mut single, PARTITION_OFFSET, (address >> 32) as u32);
    write_u32(&mut single, PARTITION_OFFSET + 4, address as u32);
    write_u32(
        &mut single,
        PARTITION_OFFSET + 8,
        (length_sectors >> 32) as u32,
    );
    write_u32(&mut single, PARTITION_OFFSET + 12, length_sectors as u32);
    single[PARTITION_OFFSET + 16..PARTITION_OFFSET + 20].copy_from_slice(b"DISK");
    single[PARTITION_OFFSET + 32..PARTITION_OFFSET + 35].copy_from_slice(b"raw");

    let mut result = Vec::new();
    result
        .try_reserve_exact(total_size)
        .map_err(|_| "Unable to allocate MBR copy data")?;
    for index in 0..copies {
        write_u32(&mut single, 20, index);
        let crc = crc32fast::hash(&single[4..]);
        write_u32(&mut single, 0, crc);
        result.extend_from_slice(&single);
    }
    Ok(result)
}

fn write_u32(buffer: &mut [u8], offset: usize, value: u32) {
    buffer[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::mbr_parser::{SunxiMbr, MBR_SIZE};
    use crate::test_support::{mbr_bytes, temp_file, test_firmware, FirmwareEntry};
    use std::path::PathBuf;

    #[test]
    fn image_lengths_round_up_to_complete_sectors() {
        assert_eq!(image_size_in_sectors(1), 1);
        assert_eq!(image_size_in_sectors(512), 1);
        assert_eq!(image_size_in_sectors(513), 2);
    }

    #[test]
    fn command_and_logic_offset_modes_build_the_reference_layouts() {
        let path = PathBuf::from("raw.img");
        let command = build_layout(
            path.clone(),
            513,
            RawMode::Command,
            RawStorage::Sdmmc,
            None,
            4,
        )
        .unwrap();
        assert_eq!(command.mbr_data.len(), MBR_SIZE);
        assert_eq!(command.partitions[0].address, 0);
        assert_eq!(command.partitions[0].data_length, 513);
        assert_eq!(command.partitions[0].path, path);
        assert!(!command.partitions[0].wrap_address);
        let command_mbr = SunxiMbr::parse(&command.mbr_data).unwrap();
        assert_eq!(command_mbr.copy, 1);
        assert_eq!(command_mbr.partitions[0].address(), 0);
        assert_eq!(command_mbr.partitions[0].length(), 2);

        let logic = build_layout(
            PathBuf::from("raw.img"),
            1024,
            RawMode::LogicOffset,
            RawStorage::Sdmmc,
            None,
            4,
        )
        .unwrap();
        assert_eq!(logic.mbr_data.len(), 4 * MBR_SIZE);
        assert_eq!(logic.partitions[0].address, 0x1_0000_0000 - 40960);
        assert!(logic.partitions[0].wrap_address);
        let logic_mbr = SunxiMbr::parse(&logic.mbr_data).unwrap();
        assert_eq!(logic_mbr.copy, 4);
        assert_eq!(logic_mbr.partitions[0].address(), 0x1_0000_0000 - 40960);
        assert_eq!(logic_mbr.partitions[0].length(), 2);
    }

    #[test]
    fn virtual_mbr_copies_have_indices_and_valid_crc32() {
        let bytes = build_virtual_mbr(0x1234, 7, 3).unwrap();
        assert_eq!(bytes.len(), 3 * MBR_SIZE);

        for (index, copy) in bytes.as_chunks::<MBR_SIZE>().0.iter().enumerate() {
            assert_eq!(u32::from_le_bytes(copy[16..20].try_into().unwrap()), 3);
            assert_eq!(
                u32::from_le_bytes(copy[20..24].try_into().unwrap()),
                index as u32
            );
            assert_eq!(
                u32::from_le_bytes(copy[..4].try_into().unwrap()),
                crc32fast::hash(&copy[4..])
            );
        }
    }

    #[test]
    fn storage_defaults_and_logic_offset_bounds_match_generic_flash() {
        assert_eq!(RawStorage::Sdmmc.default_logic_offset(), 40960);
        assert_eq!(RawStorage::Ufs.default_logic_offset(), 40960);
        assert_eq!(RawStorage::Nor.default_logic_offset(), 2106);
        assert!(build_layout(
            PathBuf::from("raw.img"),
            1,
            RawMode::LogicOffset,
            RawStorage::Sdmmc,
            Some(0x1_0000_0001),
            1,
        )
        .is_err());
    }

    #[test]
    fn prepare_inherits_boot_mbr_copies_and_sets_fixed_flash_options() {
        let mut mbr = mbr_bytes(&[]);
        mbr[16..20].copy_from_slice(&4u32.to_le_bytes());
        let crc = crc32fast::hash(&mbr[4..MBR_SIZE]);
        mbr[..4].copy_from_slice(&crc.to_le_bytes());
        let firmware = test_firmware(&[FirmwareEntry {
            filename: "sunxi_mbr.fex",
            maintype: "12345678",
            subtype: "1234567890___MBR",
            data: &mbr,
        }]);
        let image = temp_file("raw-prepare-image", b"raw image");
        let options = RawOptions {
            firmware_path: firmware.path().to_path_buf(),
            image_path: image.path().to_path_buf(),
            mode: RawMode::LogicOffset,
            storage: RawStorage::Ufs,
            logic_offset: Some(8192),
            bus: Some(2),
            port: Some(3),
            verbose: true,
        };

        let (_, request) = prepare(&options).unwrap();
        assert_eq!(request.device.selected_pair(), Some((2, 3)));
        assert_eq!(request.mode, FlashMode::FullErase);
        assert!(!request.verify);
        assert_eq!(request.post_action, PostAction::Reboot);
        let layout = request.custom_layout.unwrap();
        assert_eq!(layout.mbr_data.len(), 4 * MBR_SIZE);
        assert_eq!(layout.partitions[0].address, 0x1_0000_0000 - 8192);
    }

    #[test]
    fn prepare_rejects_missing_inputs_and_empty_raw_images() {
        let missing = RawOptions {
            firmware_path: PathBuf::from("missing-boot-firmware.img"),
            image_path: PathBuf::from("missing-raw-image.img"),
            mode: RawMode::Command,
            storage: RawStorage::Sdmmc,
            logic_offset: None,
            bus: None,
            port: None,
            verbose: false,
        };
        assert!(prepare(&missing)
            .err()
            .unwrap()
            .starts_with("Firmware file not found:"));

        let firmware = test_firmware(&[]);
        let empty = temp_file("empty-raw-image", b"");
        let empty_options = RawOptions {
            firmware_path: firmware.path().to_path_buf(),
            image_path: empty.path().to_path_buf(),
            ..missing
        };
        assert_eq!(prepare(&empty_options).err().unwrap(), "Raw image is empty");
    }
}
