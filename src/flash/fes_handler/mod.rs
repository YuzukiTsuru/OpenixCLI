//! FES (Flash Eraser Script) handler
//!
//! Handles FES mode operations for devices in U-Boot mode
//! FES mode is used for flashing partitions and boot images to storage

mod boot_download;
mod constants;
mod erase_flag;
mod mbr_download;
mod partition;
mod types;
mod ubifs_config;

pub use boot_download::BootDownload;
pub use erase_flag::EraseFlag;
pub use mbr_download::MbrDownload;
pub use partition::PartitionDownload;
pub use types::PartitionDownloadInfo;
pub use ubifs_config::UbifsConfig;

use crate::config::boot_header::get_sunxi_boot_file_mode_string;
use crate::config::mbr_parser::SunxiMbr;
use crate::firmware::{OpenixPacker, StorageType};
use crate::flash::FlashMode;
use crate::process::StageType;
use crate::utils::{FlashError, FlashResult, Logger};

/// FES handler for devices in U-Boot mode
///
/// Handles partition flashing, MBR writing, and boot image downloading
/// for devices that are in FES mode (U-Boot)
pub struct FesHandler<'a> {
    logger: &'a mut Logger,
}

impl<'a> FesHandler<'a> {
    /// Create a new FES handler
    pub fn new(logger: &'a mut Logger) -> Self {
        Self { logger }
    }

    /// Handle FES mode operations
    ///
    /// Executes the full flashing process:
    /// 1. Query device information (boot mode, storage type, flash size)
    /// 2. Erase flash if required
    /// 3. Download MBR
    /// 4. Download partitions
    /// 5. Download boot images
    pub async fn handle(
        &mut self,
        ctx: &libefex::Context,
        packer: &mut OpenixPacker,
        options: &crate::flash::FlashOptions,
    ) -> FlashResult<()> {
        self.logger.begin_stage(StageType::FesQuery);

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

        self.logger.complete_stage();

        if options.mode != FlashMode::Partition {
            self.logger.begin_stage(StageType::FesErase);
            let erase_flag = EraseFlag::new(&*self.logger);
            erase_flag.execute(ctx, options.mode).await?;
            self.logger.complete_stage();
        }

        self.logger.begin_stage(StageType::FesMbr);

        let mbr_data = packer.get_mbr().map_err(|_| FlashError::MbrNotFound)?;
        let mbr = SunxiMbr::parse(&mbr_data)
            .map_err(|e| FlashError::InvalidFirmwareFormat(e.to_string()))?;
        let mbr_info = mbr.to_mbr_info();

        self.logger
            .info(&format!("Found {} partitions in MBR", mbr_info.part_count));

        let download_list = self.prepare_partition_download_list(packer, &mbr_info, options)?;

        let ubifs_config = UbifsConfig::new(&*self.logger);
        ubifs_config.execute(
            ctx,
            &mut *packer,
            &download_list,
            StorageType::from(storage_type),
        )?;

        let mbr_download = MbrDownload::new(&*self.logger);
        mbr_download.execute(ctx, &mbr_data).await?;

        self.logger.complete_stage();

        if !download_list.is_empty() {
            self.logger.begin_stage(StageType::FesPartitions);

            let total_bytes: u64 = download_list.iter().map(|p| p.data_length).sum();
            self.logger.set_partition_stage_weight(total_bytes);

            {
                let mut partition_download = PartitionDownload::new(&mut *self.logger);
                partition_download
                    .execute(ctx, packer, &download_list, options.verify)
                    .await?;
            }

            self.logger.complete_stage();

            self.logger.begin_stage(StageType::FesBoot);
            let boot_download = BootDownload::new(&*self.logger);
            boot_download
                .execute(ctx, packer, secure, storage_type)
                .await?;
            self.logger.complete_stage();
        }

        Ok(())
    }

    /// Prepare the list of partitions to download
    ///
    /// Filters partitions based on flash mode and user-specified partition list
    fn prepare_partition_download_list(
        &self,
        packer: &mut OpenixPacker,
        mbr_info: &crate::config::mbr_parser::MbrInfo,
        options: &crate::flash::FlashOptions,
    ) -> FlashResult<Vec<PartitionDownloadInfo>> {
        use crate::config::partition::OpenixPartition;

        let mut partition_parser = OpenixPartition::new();

        if let Ok(data) = packer.get_sys_partition() {
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
