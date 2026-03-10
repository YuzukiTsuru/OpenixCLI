use crate::utils::{FlashError, FlashResult, Logger};
use libefex::FesDataType;

const UBIFS_NODE_MAGIC: u32 = 0x06101831;
const UBIFS_CHECK_BUFFER_SIZE: usize = 4096;

const SKIP_PARTITIONS: [&str; 3] = ["UDISK", "sysrecovery", "private"];

pub struct UbifsConfig<'a> {
    logger: &'a Logger,
}

impl<'a> UbifsConfig<'a> {
    pub fn new(logger: &'a Logger) -> Self {
        Self { logger }
    }

    pub fn execute(
        &self,
        ctx: &libefex::Context,
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

                self.logger
                    .info(&format!("UBIFS config set for partition {}", partition_name));
                return Ok(UbifsConfigResult::Configured {
                    partition_name: partition_name.clone(),
                });
            }
        }

        self.logger.info("No UBIFS partitions found");
        Ok(UbifsConfigResult::NotFound)
    }

    fn should_skip_partition(partition_name: &str) -> bool {
        let upper_name = partition_name.to_uppercase();
        SKIP_PARTITIONS
            .iter()
            .any(|skip| upper_name.starts_with(skip))
    }

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
                packer.get_file_data_range_by_maintype_subtype(
                    "12345678",
                    download_subtype,
                    0,
                    4,
                )
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

#[derive(Debug, Clone)]
pub enum UbifsConfigResult {
    Skipped,
    NotFound,
    Configured { partition_name: String },
}
