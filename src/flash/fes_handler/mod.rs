//! FES (Flash Eraser Script) handler
//!
//! Handles FES mode operations for devices in U-Boot mode
//! FES mode is used for flashing partitions and boot images to storage

mod boot_download;
mod constants;
mod erase_flag;
mod mbr_download;
mod partition;
mod partition_planner;
mod types;
mod ubifs_config;

pub use boot_download::BootDownload;
pub use erase_flag::EraseFlag;
pub use mbr_download::MbrDownload;
pub use partition::PartitionDownload;
pub use partition_planner::PartitionPlanner;
pub use ubifs_config::UbifsConfig;

use crate::config::boot_header::get_sunxi_boot_file_mode_string;
use crate::config::mbr_parser::{MbrInfo, SunxiMbr};
use crate::firmware::{OpenixPacker, StorageType};
use crate::flash::protocol::FesOps;
use crate::flash::{CustomFlashLayout, FlashMode, FlashRequest};
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
    pub async fn handle<C: FesOps>(
        &mut self,
        ctx: &C,
        packer: &mut OpenixPacker,
        request: &FlashRequest,
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

        if request.mode != FlashMode::Partition {
            self.logger.begin_stage(StageType::FesErase);
            let erase_flag = EraseFlag::new(&*self.logger);
            erase_flag.execute(ctx, request.mode).await?;
            self.logger.complete_stage();
        }

        self.logger.begin_stage(StageType::FesMbr);

        let mbr_data = if let Some(layout) = &request.custom_layout {
            layout.mbr_data.clone()
        } else {
            packer.get_mbr().map_err(|_| FlashError::MbrNotFound)?
        };
        let mbr = SunxiMbr::parse(&mbr_data)
            .map_err(|e| FlashError::InvalidFirmwareFormat(e.to_string()))?;
        let mbr_info = mbr.to_mbr_info();

        self.logger
            .info(&format!("Found {} partitions in MBR", mbr_info.part_count));

        let download_list = if let Some(layout) = &request.custom_layout {
            prepare_custom_downloads(layout, &mbr_info)?
        } else {
            let partition_planner = PartitionPlanner::new(&*self.logger);
            partition_planner.prepare(packer, &mbr_info, request)?
        };

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
                    .execute(ctx, packer, &download_list, request.verify)
                    .await?;
            }

            self.logger.complete_stage();
        }

        self.logger.begin_stage(StageType::FesBoot);
        let boot_download = BootDownload::new(&*self.logger);
        boot_download
            .execute(ctx, packer, secure, storage_type)
            .await?;
        self.logger.complete_stage();

        Ok(())
    }
}

fn prepare_custom_downloads(
    layout: &CustomFlashLayout,
    mbr_info: &MbrInfo,
) -> FlashResult<Vec<types::PartitionDownloadInfo>> {
    layout
        .partitions
        .iter()
        .map(|external| {
            let partition = mbr_info
                .partitions
                .iter()
                .find(|partition| partition.name == external.name)
                .ok_or_else(|| {
                    FlashError::InvalidFirmwareFormat(format!(
                        "External partition {} is missing from the custom MBR",
                        external.name
                    ))
                })?;
            if partition.address() != external.address {
                return Err(FlashError::InvalidFirmwareFormat(format!(
                    "External partition {} address does not match the custom MBR",
                    external.name
                )));
            }
            let capacity = partition.length().checked_mul(512).ok_or_else(|| {
                FlashError::InvalidFirmwareFormat(format!(
                    "External partition {} capacity is too large",
                    external.name
                ))
            })?;
            if external.data_length > capacity {
                return Err(FlashError::InvalidFirmwareFormat(format!(
                    "External partition {} data exceeds its MBR capacity",
                    external.name
                )));
            }

            Ok(types::PartitionDownloadInfo {
                partition_name: external.name.clone(),
                partition_address: external.address,
                download_filename: external.path.display().to_string(),
                download_subtype: String::new(),
                data_offset: 0,
                data_length: external.data_length,
                source: types::PartitionSource::ExternalFile(external.path.clone()),
                wrap_address: external.wrap_address,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flash::protocol::tests::MockProtocol;
    use crate::flash::{CustomFlashLayout, ExternalPartition};
    use crate::flash::{DeviceSelector, PostAction};
    use crate::test_support::{mbr_bytes, test_firmware, FirmwareEntry};
    use libefex::FesDataType;

    fn request(mode: FlashMode) -> FlashRequest {
        FlashRequest::new(
            DeviceSelector::default(),
            false,
            mode,
            None,
            PostAction::Reboot,
        )
    }

    fn complete_firmware() -> crate::test_support::TestFile {
        let mbr = mbr_bytes(&[("system", 0x20, 3, false)]);
        test_firmware(&[
            FirmwareEntry {
                filename: "sunxi_mbr.fex",
                maintype: "12345678",
                subtype: "1234567890___MBR",
                data: &mbr,
            },
            FirmwareEntry {
                filename: "sys_partition.fex",
                maintype: "COMMON",
                subtype: "SYS_CONFIG000000",
                data: b"[partition_start]\n[partition]\nname=system\ndownloadfile=system.img\n",
            },
            FirmwareEntry {
                filename: "system.img",
                maintype: "RFSFAT16",
                subtype: "SYSTEM_IMG000000",
                data: b"raw",
            },
            FirmwareEntry {
                filename: "preboot.fex",
                maintype: "12345678",
                subtype: "1234567890PREB_0",
                data: b"preboot",
            },
            FirmwareEntry {
                filename: "uboot.fex",
                maintype: "12345678",
                subtype: "UBOOT_0000000000",
                data: b"boot1",
            },
            FirmwareEntry {
                filename: "boot0.fex",
                maintype: "12345678",
                subtype: "1234567890BUFS_0",
                data: b"boot0",
            },
        ])
    }

    #[tokio::test]
    async fn full_handler_orders_erase_mbr_partition_preboot_boot1_boot0() {
        let firmware = complete_firmware();
        let mut packer = OpenixPacker::new();
        packer.load(firmware.path()).unwrap();
        let mut logger = Logger::for_events(false, crate::flash::FlashEventSink::none());
        let ctx = MockProtocol::default();
        FesHandler::new(&mut logger)
            .handle(&ctx, &mut packer, &request(FlashMode::FullErase))
            .await
            .unwrap();

        let types: Vec<_> = ctx
            .downloads
            .borrow()
            .iter()
            .map(|download| download.data_type)
            .collect();
        assert_eq!(
            types,
            [
                FesDataType::Erase,
                FesDataType::Mbr,
                FesDataType::Flash,
                FesDataType::Preboot,
                FesDataType::Boot1,
                FesDataType::Boot0,
            ]
        );
        assert_eq!(&*ctx.flash_switches.borrow(), &[(0, true), (0, false)]);
    }

    #[tokio::test]
    async fn partition_mode_skips_erase_stage() {
        let firmware = complete_firmware();
        let mut packer = OpenixPacker::new();
        packer.load(firmware.path()).unwrap();
        let mut logger = Logger::for_events(false, crate::flash::FlashEventSink::none());
        let ctx = MockProtocol::default();
        FesHandler::new(&mut logger)
            .handle(&ctx, &mut packer, &request(FlashMode::Partition))
            .await
            .unwrap();
        assert_ne!(ctx.downloads.borrow()[0].data_type, FesDataType::Erase);
    }

    #[tokio::test]
    async fn custom_layout_replaces_the_firmware_mbr_and_partition_source() {
        let raw = crate::test_support::temp_file("fes-custom-raw", b"raw payload");
        let firmware = complete_firmware();
        let mut packer = OpenixPacker::new();
        packer.load(firmware.path()).unwrap();
        let custom_mbr = crate::raw::build_virtual_mbr(0x40, 1, 1).unwrap();
        let request = request(FlashMode::FullErase).with_custom_layout(CustomFlashLayout::new(
            custom_mbr.clone(),
            vec![ExternalPartition::new("raw", raw.path(), 0x40, 11)],
        ));
        let mut logger = Logger::for_events(false, crate::flash::FlashEventSink::none());
        let ctx = MockProtocol::default();

        FesHandler::new(&mut logger)
            .handle(&ctx, &mut packer, &request)
            .await
            .unwrap();

        let downloads = ctx.downloads.borrow();
        let mbr = downloads
            .iter()
            .find(|download| download.data_type == FesDataType::Mbr)
            .unwrap();
        assert_eq!(mbr.data, custom_mbr);
        let raw_download = downloads
            .iter()
            .find(|download| download.data_type == FesDataType::Flash)
            .unwrap();
        assert_eq!(raw_download.addr, 0x40);
        assert_eq!(raw_download.data, b"raw payload");
    }

    #[tokio::test]
    async fn boot_images_are_written_when_there_are_no_regular_partitions() {
        let mbr = mbr_bytes(&[]);
        let firmware = test_firmware(&[
            FirmwareEntry {
                filename: "sunxi_mbr.fex",
                maintype: "12345678",
                subtype: "1234567890___MBR",
                data: &mbr,
            },
            FirmwareEntry {
                filename: "preboot.fex",
                maintype: "12345678",
                subtype: "1234567890PREB_0",
                data: b"preboot",
            },
            FirmwareEntry {
                filename: "uboot.fex",
                maintype: "12345678",
                subtype: "UBOOT_0000000000",
                data: b"boot1",
            },
            FirmwareEntry {
                filename: "boot0.fex",
                maintype: "12345678",
                subtype: "1234567890BUFS_0",
                data: b"boot0",
            },
        ]);
        let mut packer = OpenixPacker::new();
        packer.load(firmware.path()).unwrap();
        let mut logger = Logger::for_events(false, crate::flash::FlashEventSink::none());
        let ctx = MockProtocol::default();

        FesHandler::new(&mut logger)
            .handle(&ctx, &mut packer, &request(FlashMode::FullErase))
            .await
            .unwrap();

        let types: Vec<_> = ctx
            .downloads
            .borrow()
            .iter()
            .map(|download| download.data_type)
            .collect();
        assert_eq!(
            types,
            [
                FesDataType::Erase,
                FesDataType::Mbr,
                FesDataType::Preboot,
                FesDataType::Boot1,
                FesDataType::Boot0,
            ]
        );
    }

    #[tokio::test]
    async fn query_and_mbr_errors_are_propagated() {
        for failure in ["secure", "storage", "size"] {
            let firmware = complete_firmware();
            let mut packer = OpenixPacker::new();
            packer.load(firmware.path()).unwrap();
            let mut logger = Logger::for_events(false, crate::flash::FlashEventSink::none());
            let ctx = MockProtocol::default();
            match failure {
                "secure" => *ctx.secure.borrow_mut() = Err("secure".to_string()),
                "storage" => *ctx.storage.borrow_mut() = Err("storage".to_string()),
                "size" => *ctx.flash_size.borrow_mut() = Err("size".to_string()),
                _ => unreachable!(),
            }
            assert!(matches!(
                FesHandler::new(&mut logger)
                    .handle(&ctx, &mut packer, &request(FlashMode::FullErase))
                    .await,
                Err(FlashError::UsbTransferError(_))
            ));
        }

        let no_mbr = test_firmware(&[]);
        let mut packer = OpenixPacker::new();
        packer.load(no_mbr.path()).unwrap();
        let mut logger = Logger::for_events(false, crate::flash::FlashEventSink::none());
        assert!(matches!(
            FesHandler::new(&mut logger)
                .handle(
                    &MockProtocol::default(),
                    &mut packer,
                    &request(FlashMode::FullErase)
                )
                .await,
            Err(FlashError::MbrNotFound)
        ));
    }
}
