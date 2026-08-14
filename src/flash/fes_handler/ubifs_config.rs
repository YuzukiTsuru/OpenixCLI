//! UBIFS configuration handler
//!
//! Handles UBIFS (UBI File System) configuration for NAND partitions

use crate::flash::protocol::FesOps;
use crate::utils::{FlashError, FlashResult, Logger};
use libefex::FesDataType;

/// UBIFS node magic number
const UBIFS_NODE_MAGIC: u32 = 0x06101831;
/// Buffer size for UBIFS checking
const UBIFS_CHECK_BUFFER_SIZE: usize = 4096;

/// Partitions to skip during UBIFS configuration
const SKIP_PARTITIONS: [&str; 3] = ["UDISK", "SYSRECOVERY", "PRIVATE"];

/// UBIFS configuration handler
///
/// Detects UBIFS partitions and configures them for NAND storage
pub struct UbifsConfig<'a> {
    logger: &'a Logger,
}

impl<'a> UbifsConfig<'a> {
    /// Create a new UBIFS config handler
    pub fn new(logger: &'a Logger) -> Self {
        Self { logger }
    }

    /// Execute UBIFS configuration
    ///
    /// Checks partitions for UBIFS magic and configures them if found
    /// Returns early after first UBIFS partition is found
    pub fn execute<C: FesOps>(
        &self,
        ctx: &C,
        packer: &mut crate::firmware::OpenixPacker,
        download_list: &[super::types::PartitionDownloadInfo],
        storage_type: crate::firmware::StorageType,
    ) -> FlashResult<UbifsConfigResult> {
        if storage_type == crate::firmware::StorageType::Sdcard
            || storage_type == crate::firmware::StorageType::Sd1
        {
            self.logger
                .info("Skipping UBIFS config for SD card storage");
            return Ok(UbifsConfigResult::Skipped);
        }

        if download_list.is_empty() {
            self.logger.info("No partitions to check for UBIFS");
            return Ok(UbifsConfigResult::Skipped);
        }

        for partition_info in download_list {
            let partition_name = &partition_info.partition_name;

            if Self::should_skip_partition(partition_name) {
                continue;
            }

            self.logger
                .debug(&format!("Checking partition {} for UBIFS", partition_name));

            if self.check_ubifs_magic(&mut *packer, &partition_info.download_subtype)? {
                self.logger
                    .info(&format!("Found UBIFS partition: {}", partition_name));

                let buffer = vec![0u8; UBIFS_CHECK_BUFFER_SIZE];
                ctx.fes_down(&buffer, 0, FesDataType::Ext4Ubifs)
                    .map_err(|e| FlashError::UsbTransferError(e.to_string()))?;

                self.logger.info(&format!(
                    "UBIFS config set for partition {}",
                    partition_name
                ));
                return Ok(UbifsConfigResult::Configured {
                    partition_name: partition_name.clone(),
                });
            }
        }

        self.logger.info("No UBIFS partitions found");
        Ok(UbifsConfigResult::NotFound)
    }

    /// Check if partition should be skipped
    fn should_skip_partition(partition_name: &str) -> bool {
        let upper_name = partition_name.to_uppercase();
        SKIP_PARTITIONS
            .iter()
            .any(|skip| upper_name.starts_with(skip))
    }

    /// Check if partition data starts with UBIFS magic
    fn check_ubifs_magic(
        &self,
        packer: &mut crate::firmware::OpenixPacker,
        download_subtype: &str,
    ) -> FlashResult<bool> {
        let data = packer
            .get_file_data_range_by_maintype_subtype(
                super::types::ITEM_ROOTFSFAT16,
                download_subtype,
                0,
                4,
            )
            .or_else(|_| {
                packer.get_file_data_range_by_maintype_subtype("12345678", download_subtype, 0, 4)
            });

        match data {
            Ok(data) if data.len() >= 4 => {
                let magic = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                Ok(magic == UBIFS_NODE_MAGIC)
            }
            _ => Ok(false),
        }
    }
}

/// Result of UBIFS configuration operation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UbifsConfigResult {
    /// Skipped (e.g., SD card storage)
    Skipped,
    /// No UBIFS partitions found
    NotFound,
    /// UBIFS configured for a partition
    Configured { partition_name: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flash::fes_handler::types::PartitionDownloadInfo;
    use crate::flash::protocol::tests::MockProtocol;
    use crate::test_support::{test_firmware, FirmwareEntry};

    fn partition(name: &str, subtype: &str) -> PartitionDownloadInfo {
        PartitionDownloadInfo {
            partition_name: name.to_string(),
            partition_address: 0,
            download_filename: "image.bin".to_string(),
            download_subtype: subtype.to_string(),
            data_offset: 0,
            data_length: 4,
        }
    }

    #[test]
    fn skip_matching_is_case_insensitive_and_prefix_based() {
        for name in ["UDISK", "udisk_data", "SysRecovery", "private_data"] {
            assert!(UbifsConfig::should_skip_partition(name), "{name}");
        }
        assert!(!UbifsConfig::should_skip_partition("system"));
    }

    #[test]
    fn sd_storage_and_empty_lists_are_skipped_without_io() {
        let firmware = test_firmware(&[]);
        let mut packer = crate::firmware::OpenixPacker::new();
        packer.load(firmware.path()).unwrap();
        let logger = Logger::for_events(false, crate::flash::FlashEventSink::none());
        let handler = UbifsConfig::new(&logger);
        let ctx = MockProtocol::default();
        let item = partition("system", "SYSTEM0000000000");
        assert_eq!(
            handler
                .execute(
                    &ctx,
                    &mut packer,
                    std::slice::from_ref(&item),
                    crate::firmware::StorageType::Sdcard
                )
                .unwrap(),
            UbifsConfigResult::Skipped
        );
        assert_eq!(
            handler
                .execute(&ctx, &mut packer, &[], crate::firmware::StorageType::Nand)
                .unwrap(),
            UbifsConfigResult::Skipped
        );
        assert!(ctx.downloads.borrow().is_empty());
    }

    #[test]
    fn detects_ubifs_in_primary_and_fallback_maintypes() {
        let magic = UBIFS_NODE_MAGIC.to_le_bytes();
        for maintype in [super::super::types::ITEM_ROOTFSFAT16, "12345678"] {
            let firmware = test_firmware(&[FirmwareEntry {
                filename: "system.img",
                maintype,
                subtype: "SYSTEM0000000000",
                data: &magic,
            }]);
            let mut packer = crate::firmware::OpenixPacker::new();
            packer.load(firmware.path()).unwrap();
            let logger = Logger::for_events(false, crate::flash::FlashEventSink::none());
            let ctx = MockProtocol::default();
            let result = UbifsConfig::new(&logger)
                .execute(
                    &ctx,
                    &mut packer,
                    &[partition("system", "SYSTEM0000000000")],
                    crate::firmware::StorageType::Nand,
                )
                .unwrap();
            assert_eq!(
                result,
                UbifsConfigResult::Configured {
                    partition_name: "system".to_string()
                }
            );
            let downloads = ctx.downloads.borrow();
            assert_eq!(downloads.len(), 1);
            assert_eq!(downloads[0].data_type, FesDataType::Ext4Ubifs);
            assert_eq!(downloads[0].data.len(), UBIFS_CHECK_BUFFER_SIZE);
        }
    }

    #[test]
    fn skipped_missing_short_and_non_ubifs_entries_are_not_configured() {
        let firmware = test_firmware(&[
            FirmwareEntry {
                filename: "skip.img",
                maintype: "RFSFAT16",
                subtype: "SKIP000000000000",
                data: &UBIFS_NODE_MAGIC.to_le_bytes(),
            },
            FirmwareEntry {
                filename: "short.img",
                maintype: "RFSFAT16",
                subtype: "SHORT00000000000",
                data: &[1, 2, 3],
            },
            FirmwareEntry {
                filename: "raw.img",
                maintype: "RFSFAT16",
                subtype: "RAW0000000000000",
                data: &[0, 0, 0, 0],
            },
        ]);
        let mut packer = crate::firmware::OpenixPacker::new();
        packer.load(firmware.path()).unwrap();
        let logger = Logger::for_events(false, crate::flash::FlashEventSink::none());
        let ctx = MockProtocol::default();
        let list = [
            partition("private_data", "SKIP000000000000"),
            partition("short", "SHORT00000000000"),
            partition("raw", "RAW0000000000000"),
            partition("missing", "MISSING000000000"),
        ];
        assert_eq!(
            UbifsConfig::new(&logger)
                .execute(&ctx, &mut packer, &list, crate::firmware::StorageType::Nand)
                .unwrap(),
            UbifsConfigResult::NotFound
        );
        assert!(ctx.downloads.borrow().is_empty());
    }

    #[test]
    fn configuration_transfer_errors_are_propagated() {
        let magic = UBIFS_NODE_MAGIC.to_le_bytes();
        let firmware = test_firmware(&[FirmwareEntry {
            filename: "system.img",
            maintype: "RFSFAT16",
            subtype: "SYSTEM0000000000",
            data: &magic,
        }]);
        let mut packer = crate::firmware::OpenixPacker::new();
        packer.load(firmware.path()).unwrap();
        let logger = Logger::for_events(false, crate::flash::FlashEventSink::none());
        let ctx = MockProtocol::default();
        *ctx.fail_down.borrow_mut() = Some("down".to_string());
        assert!(matches!(
            UbifsConfig::new(&logger).execute(
                &ctx,
                &mut packer,
                &[partition("system", "SYSTEM0000000000")],
                crate::firmware::StorageType::Nand
            ),
            Err(FlashError::UsbTransferError(_))
        ));
    }
}
