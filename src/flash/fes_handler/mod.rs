mod boot_download;
mod erase_flag;
mod mbr_download;
mod partition_download;
mod types;

pub use boot_download::BootDownload;
pub use erase_flag::EraseFlag;
pub use mbr_download::MbrDownload;
pub use partition_download::PartitionDownload;
pub use types::PartitionDownloadInfo;

use crate::config::boot_header::get_sunxi_boot_file_mode_string;
use crate::config::mbr_parser::SunxiMbr;
use crate::firmware::{OpenixPacker, StorageType};
use crate::flash::FlashMode;
use crate::utils::{FlashError, FlashResult, Logger};

pub struct FesHandler<'a> {
    logger: &'a mut Logger,
}

impl<'a> FesHandler<'a> {
    pub fn new(logger: &'a mut Logger) -> Self {
        Self { logger }
    }

    pub async fn handle(
        &mut self,
        ctx: &libefex::Context,
        packer: &mut OpenixPacker,
        options: &crate::flash::FlashOptions,
    ) -> FlashResult<()> {
        let secure = ctx
            .fes_query_secure()
            .map_err(|e| FlashError::UsbTransferError(e.to_string()))?;
        self.logger.info(&format!(
            "Boot mode: {}",
            get_sunxi_boot_file_mode_string(secure)
        ));

        let storage_type = ctx
            .fes_query_storage()
            .map_err(|e| FlashError::UsbTransferError(e.to_string()))?;
        self.logger.info(&format!(
            "Storage type: {}",
            StorageType::from(storage_type)
        ));

        let flash_size = ctx
            .fes_probe_flash_size()
            .map_err(|e| FlashError::UsbTransferError(e.to_string()))?;
        self.logger.info(&format!(
            "Flash size: {} MB",
            (flash_size as u64) * 512 / 1024 / 1024
        ));

        if options.mode != FlashMode::Partition {
            let erase_flag = EraseFlag::new(&*self.logger);
            erase_flag.execute(ctx, options.mode).await?;
        }

        let mbr_data = packer.get_mbr().map_err(|_| FlashError::MbrNotFound)?;
        let mbr = SunxiMbr::parse(&mbr_data)
            .map_err(|e| FlashError::InvalidFirmwareFormat(e.to_string()))?;
        let mbr_info = mbr.to_mbr_info();

        self.logger
            .info(&format!("Found {} partitions in MBR", mbr_info.part_count));

        let mbr_download = MbrDownload::new(&*self.logger);
        mbr_download.execute(ctx, &mbr_data).await?;

        let download_list = self.prepare_partition_download_list(packer, &mbr_info, options)?;
        if !download_list.is_empty() {
            {
                let mut partition_download = PartitionDownload::new(&mut *self.logger);
                partition_download
                    .execute(ctx, packer, &download_list, options.verify)
                    .await?;
            }
            let boot_download = BootDownload::new(&*self.logger);
            boot_download
                .execute(ctx, packer, secure, storage_type)
                .await?;
        }

        Ok(())
    }

    fn prepare_partition_download_list(
        &self,
        packer: &mut OpenixPacker,
        mbr_info: &crate::config::mbr_parser::MbrInfo,
        options: &crate::flash::FlashOptions,
    ) -> FlashResult<Vec<PartitionDownloadInfo>> {
        use crate::config::partition::OpenixPartition;

        let mut partition_parser = OpenixPartition::new();

        let partition_data = packer
            .get_file_data_by_filename("sys_partition.bin")
            .or_else(|_| packer.get_file_data_by_filename("sys_partition.fex"));

        if let Ok(data) = partition_data {
            partition_parser.parse_from_data(&data);
        }

        let config_partitions = partition_parser.get_partitions();
        let mut download_list = Vec::new();

        for mbr_partition in &mbr_info.partitions {
            let partition_name = &mbr_partition.name;

            if options.mode == FlashMode::KeepData {
                let name_lower = partition_name.to_lowercase();
                if name_lower == "udisk" || name_lower == "private" || name_lower == "reserve" {
                    self.logger
                        .info(&format!("Skipping user data partition: {}", partition_name));
                    continue;
                }
            }

            if options.mode == FlashMode::Partition {
                if let Some(ref partitions) = options.partitions {
                    if !partitions.iter().any(|p| p == partition_name) {
                        self.logger.info(&format!(
                            "Skipping partition not in list: {}",
                            partition_name
                        ));
                        continue;
                    }
                }
            }

            let config_partition = config_partitions.iter().find(|p| p.name == *partition_name);

            let download_filename = match config_partition {
                Some(cp) if !cp.downloadfile.is_empty() => cp.downloadfile.clone(),
                _ => {
                    self.logger.debug(&format!(
                        "Partition {} has no download file, skipping",
                        partition_name
                    ));
                    continue;
                }
            };

            let download_subtype = packer.build_subtype_by_filename(&download_filename);

            let data_info = packer
                .get_file_info_by_maintype_subtype(types::ITEM_ROOTFSFAT16, &download_subtype)
                .or_else(|| packer.get_file_info_by_maintype_subtype("12345678", &download_subtype))
                .or_else(|| packer.get_file_info_by_filename(&download_filename));

            if let Some((offset, length)) = data_info {
                download_list.push(PartitionDownloadInfo {
                    partition_name: partition_name.clone(),
                    partition_address: mbr_partition.address(),
                    download_filename,
                    download_subtype,
                    data_offset: offset,
                    data_length: length,
                });
            } else {
                self.logger.warn(&format!(
                    "Partition image not found: {} ({})",
                    partition_name, download_filename
                ));
            }
        }

        Ok(download_list)
    }
}
