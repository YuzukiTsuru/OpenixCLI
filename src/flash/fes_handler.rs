use crate::config::boot_header::{
    get_sunxi_boot_file_mode_string, BOOT_FILE_MODE_NORMAL, BOOT_FILE_MODE_PKG, BOOT_FILE_MODE_TOC,
};
use crate::config::mbr_parser::{is_valid_mbr, SunxiMbr, EFEX_CRC32_VALID_FLAG};
use crate::config::partition::OpenixPartition;
use crate::firmware::{StorageType, OpenixPacker, PackerError};
use crate::utils::{FlashError, FlashResult, Logger};
use crate::flash::FlashMode;
use libefex::FesDataType;
use std::time::Duration;

const MAX_VERIFY_RETRIES: usize = 5;
const CHUNK_SIZE: u64 = 256 * 1024 * 1024;
const ITEM_ROOTFSFAT16: &str = "RFSFAT16";

struct IncrementalChecksum {
    sum: u32,
    pending_bytes: Vec<u8>,
}

impl IncrementalChecksum {
    fn new() -> Self {
        IncrementalChecksum {
            sum: 0,
            pending_bytes: Vec::new(),
        }
    }

    fn update(&mut self, data: &[u8]) {
        let buffer = if !self.pending_bytes.is_empty() {
            let mut combined = self.pending_bytes.clone();
            combined.extend_from_slice(data);
            self.pending_bytes.clear();
            combined
        } else {
            data.to_vec()
        };

        let aligned_length = buffer.len() & !0x03;
        let remaining = buffer.len() & 0x03;

        for i in (0..aligned_length).step_by(4) {
            let value =
                u32::from_le_bytes([buffer[i], buffer[i + 1], buffer[i + 2], buffer[i + 3]]);
            self.sum = self.sum.wrapping_add(value);
        }

        if remaining > 0 {
            self.pending_bytes = buffer[aligned_length..].to_vec();
        }
    }

    fn finalize(&mut self) -> u32 {
        if !self.pending_bytes.is_empty() {
            let last_value: u32 = match self.pending_bytes.len() {
                1 => self.pending_bytes[0] as u32 & 0x000000ff,
                2 => {
                    (self.pending_bytes[0] as u32 | (self.pending_bytes[1] as u32) << 8)
                        & 0x0000ffff
                }
                3 => {
                    (self.pending_bytes[0] as u32
                        | (self.pending_bytes[1] as u32) << 8
                        | (self.pending_bytes[2] as u32) << 16)
                        & 0x00ffffff
                }
                _ => 0,
            };
            self.sum = self.sum.wrapping_add(last_value);
            self.pending_bytes.clear();
        }
        self.sum
    }
}

#[allow(dead_code)]
struct PartitionDownloadInfo {
    partition_name: String,
    partition_address: u64,
    download_filename: String,
    download_subtype: String,
    data_offset: u64,
    data_length: u64,
}

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
            self.download_erase_flag(ctx, options.mode).await?;
        }

        let mbr_data = packer.get_mbr().map_err(|_| FlashError::MbrNotFound)?;
        let mbr = SunxiMbr::parse(&mbr_data)
            .map_err(|e| FlashError::InvalidFirmwareFormat(e.to_string()))?;
        let mbr_info = mbr.to_mbr_info();

        self.logger.info(&format!(
            "Found {} partitions in MBR",
            mbr_info.part_count
        ));

        self.download_mbr(ctx, &mbr_data).await?;

        let download_list = self.prepare_partition_download_list(packer, &mbr_info, options)?;
        if !download_list.is_empty() {
            self.download_partitions(ctx, packer, &download_list, options.verify).await?;
            self.download_boot0_boot1(ctx, packer, secure, storage_type).await?;
        }

        Ok(())
    }

    fn prepare_partition_download_list(
        &self,
        packer: &mut OpenixPacker,
        mbr_info: &crate::config::mbr_parser::MbrInfo,
        options: &crate::flash::FlashOptions,
    ) -> FlashResult<Vec<PartitionDownloadInfo>> {
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
                if name_lower == "udisk"
                    || name_lower == "private"
                    || name_lower == "reserve"
                {
                    self.logger.info(&format!(
                        "Skipping user data partition: {}",
                        partition_name
                    ));
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

            let config_partition = config_partitions
                .iter()
                .find(|p| p.name == *partition_name);

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
                .get_file_info_by_maintype_subtype(ITEM_ROOTFSFAT16, &download_subtype)
                .or_else(|| {
                    packer.get_file_info_by_maintype_subtype("12345678", &download_subtype)
                })
                .or_else(|| {
                    packer.get_file_info_by_filename(&download_filename)
                });

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
                    partition_name,
                    download_filename
                ));
            }
        }

        Ok(download_list)
    }

    async fn download_erase_flag(&self, ctx: &libefex::Context, mode: FlashMode) -> FlashResult<()> {
        self.logger.info("Downloading erase flag...");

        let mut erase_data = vec![0u8; 16];
        let erase_flag = mode.erase_flag();
        erase_data[0..4].copy_from_slice(&erase_flag.to_le_bytes());

        ctx.fes_down(&erase_data, 0, FesDataType::Erase)
            .map_err(|e| FlashError::UsbTransferError(e.to_string()))?;

        self.logger.stage_complete("Erase flag downloaded");
        Ok(())
    }

    async fn download_mbr(&self, ctx: &libefex::Context, mbr_data: &[u8]) -> FlashResult<()> {
        self.logger.info(&format!("Downloading MBR ({} bytes)...", mbr_data.len()));

        if !is_valid_mbr(mbr_data) {
            return Err(FlashError::InvalidFirmwareFormat("Invalid MBR".to_string()));
        }

        ctx.fes_down(mbr_data, 0, FesDataType::Mbr)
            .map_err(|e| FlashError::UsbTransferError(e.to_string()))?;

        for _ in 0..MAX_VERIFY_RETRIES {
            tokio::time::sleep(Duration::from_millis(100)).await;

            let verify_resp = ctx
                .fes_verify_status(0x7f01)
                .map_err(|e| FlashError::UsbTransferError(e.to_string()))?;

            if verify_resp.flag == EFEX_CRC32_VALID_FLAG
                && verify_resp.media_crc == 0
            {
                self.logger.stage_complete("MBR verified successfully");
                return Ok(());
            }
        }

        self.logger.warn("MBR verification not confirmed, continuing...");
        Ok(())
    }

    async fn download_partitions(
        &mut self,
        ctx: &libefex::Context,
        packer: &mut OpenixPacker,
        download_list: &[PartitionDownloadInfo],
        verify: bool,
    ) -> FlashResult<()> {
        if download_list.is_empty() {
            self.logger.warn("No partitions to download");
            self.logger.stage_complete("All partitions flashed (0 bytes written)");
            return Ok(());
        }

        self.logger.info(&format!("Flashing {} partitions...", download_list.len()));

        self.logger.info("Turning on flash access...");
        ctx.fes_flash_set_onoff(0, true)
            .map_err(|e| FlashError::UsbTransferError(e.to_string()))?;

        let result = self.download_partitions_inner(ctx, packer, download_list, verify).await;

        self.logger.info("Turning off flash access...");
        if let Err(e) = ctx.fes_flash_set_onoff(0, false) {
            self.logger.warn(&format!("Failed to turn off flash access: {}", e));
        }

        result
    }

    async fn download_partitions_inner(
        &mut self,
        ctx: &libefex::Context,
        packer: &mut OpenixPacker,
        download_list: &[PartitionDownloadInfo],
        verify: bool,
    ) -> FlashResult<()> {
        let total_bytes: u64 = download_list.iter().map(|p| p.data_length).sum();
        let mut written_bytes: u64 = 0;

        if total_bytes > 0 {
            self.logger.start_global_progress(total_bytes, "Initializing...");
        }

        for info in download_list {
            self.logger.info(&format!(
                "Flashing partition: {} ({} bytes at sector {})",
                info.partition_name,
                info.data_length,
                info.partition_address
            ));

            let start_sector = info.partition_address as u32;
            let total_chunks = info.data_length.div_ceil(CHUNK_SIZE);
            let mut checksum = if verify {
                Some(IncrementalChecksum::new())
            } else {
                None
            };

            for chunk_index in 0..total_chunks {
                let chunk_offset = (chunk_index * CHUNK_SIZE) as usize;
                let chunk_size = std::cmp::min(
                    CHUNK_SIZE as usize,
                    (info.data_length as usize).saturating_sub(chunk_offset),
                );

                let chunk_data = packer
                    .get_file_data_range_by_maintype_subtype(
                        ITEM_ROOTFSFAT16,
                        &info.download_subtype,
                        chunk_offset as u64,
                        chunk_size as u64,
                    )
                    .or_else(|_| {
                        packer.get_file_data_range_by_maintype_subtype(
                            "12345678",
                            &info.download_subtype,
                            chunk_offset as u64,
                            chunk_size as u64,
                        )
                    });

                let chunk_data = match chunk_data {
                    Ok(data) => data,
                    Err(e) => {
                        self.logger.warn(&format!(
                            "Failed to read chunk data for {}: {}",
                            info.partition_name, e
                        ));
                        break;
                    }
                };

                if let Some(ref mut cs) = checksum {
                    cs.update(&chunk_data);
                }

                let chunk_start_sector = start_sector.wrapping_add((chunk_offset / 512) as u32);
                let partition_name = info.partition_name.clone();
                let chunk_base_bytes = written_bytes as usize;

                ctx.fes_down_with_progress(
                    &chunk_data,
                    chunk_start_sector,
                    FesDataType::Flash,
                    {
                        let logger = &*self.logger;
                        move |transferred, _total| {
                            let current = chunk_base_bytes + transferred as usize;
                            logger.progress_update(current, &partition_name);
                        }
                    },
                )
                .map_err(|e| FlashError::UsbTransferError(e.to_string()))?;

                written_bytes += chunk_size as u64;
            }

            if verify {
                self.logger.info(&format!("Verifying partition {}...", info.partition_name));
                let local_checksum = checksum.as_mut().map(|cs| cs.finalize()).unwrap_or(0);

                let verify_resp = ctx
                    .fes_verify_value(info.partition_address as u32, info.data_length)
                    .map_err(|e| FlashError::UsbTransferError(e.to_string()))?;

                if verify_resp.flag == EFEX_CRC32_VALID_FLAG {
                    let media_crc = verify_resp.media_crc as u32;
                    if local_checksum != media_crc {
                        self.logger.warn(&format!(
                            "Partition {} checksum mismatch: local=0x{:x}, device=0x{:x}",
                            info.partition_name, local_checksum, media_crc
                        ));
                    } else {
                        self.logger.stage_complete(&format!("Partition {} verified", info.partition_name));
                    }
                } else {
                    self.logger.warn(&format!("Partition {} verification failed", info.partition_name));
                }
            } else {
                self.logger.stage_complete(&format!("Partition {} flashed", info.partition_name));
            }
        }

        self.logger.stage_complete(&format!(
            "All partitions flashed ({} bytes written)",
            written_bytes
        ));
        Ok(())
    }

    async fn download_boot0_boot1(
        &self,
        ctx: &libefex::Context,
        packer: &mut OpenixPacker,
        secure: u32,
        storage_type: u32,
    ) -> FlashResult<()> {
        self.logger.info("Downloading Boot0/Boot1...");

        let boot1_info = self.get_boot1_subtype(secure, storage_type);
        if let Some((maintype, subtype)) = boot1_info {
            self.logger.debug(&format!(
                "Looking for Boot1: {}/{}",
                maintype, subtype
            ));
            match packer.get_file_data_by_maintype_subtype(maintype, subtype) {
                Ok(boot1_data) => {
                    self.logger.info(&format!(
                        "Downloading Boot1: {}/{} ({} bytes)",
                        maintype,
                        subtype,
                        boot1_data.len()
                    ));

                    ctx.fes_down(&boot1_data, 0, FesDataType::Boot1)
                        .map_err(|e| FlashError::UsbTransferError(e.to_string()))?;

                    let verify = ctx
                        .fes_verify_status(0x7f03)
                        .map_err(|e| FlashError::UsbTransferError(e.to_string()))?;
                    if verify.flag == EFEX_CRC32_VALID_FLAG {
                        self.logger.stage_complete("Boot1 verified");
                    }
                }
                Err(e) => {
                    self.logger.debug(&format!(
                        "Boot1 not found: {}/{} - {}",
                        maintype, subtype, e
                    ));
                }
            }
        }

        let boot0_info = self.get_boot0_subtype(secure, storage_type);
        if let Some((maintype, subtype)) = boot0_info {
            self.logger.debug(&format!(
                "Looking for Boot0: {}/{}",
                maintype, subtype
            ));
            let boot0_data = packer
                .get_file_data_by_maintype_subtype(maintype, subtype)
                .or_else(|_| {
                    if let Some((m, s)) = self.get_boot0_subtype(secure, 0) {
                        packer.get_file_data_by_maintype_subtype(m, s)
                    } else {
                        Err(PackerError::FileNotFound(subtype.to_string()))
                    }
                });

            if let Ok(boot0_data) = boot0_data {
                self.logger.info(&format!(
                    "Downloading Boot0: {}/{} ({} bytes)",
                    maintype,
                    subtype,
                    boot0_data.len()
                ));

                ctx.fes_down(&boot0_data, 0, FesDataType::Boot0)
                    .map_err(|e| FlashError::UsbTransferError(e.to_string()))?;

                let verify = ctx
                    .fes_verify_status(0x7f04)
                    .map_err(|e| FlashError::UsbTransferError(e.to_string()))?;
                if verify.flag == EFEX_CRC32_VALID_FLAG {
                    self.logger.stage_complete("Boot0 verified");
                }
            }
        }

        self.logger.stage_complete("Boot0/Boot1 downloaded");
        Ok(())
    }

    fn get_boot1_subtype(&self, secure: u32, storage_type: u32) -> Option<(&'static str, &'static str)> {
        match secure {
            BOOT_FILE_MODE_NORMAL => Some(("12345678", "UBOOT_0000000000")),
            BOOT_FILE_MODE_TOC => Some(("12345678", "TOC1_00000000000")),
            BOOT_FILE_MODE_PKG => {
                if StorageType::from(storage_type) == StorageType::Spinor {
                    Some(("12345678", "BOOTPKG-NOR00000"))
                } else {
                    Some(("12345678", "BOOTPKG-00000000"))
                }
            }
            _ => None,
        }
    }

    fn get_boot0_subtype(&self, secure: u32, storage_type: u32) -> Option<(&'static str, &'static str)> {
        if secure == BOOT_FILE_MODE_NORMAL || secure == BOOT_FILE_MODE_PKG {
            match StorageType::from(storage_type) {
                StorageType::Nand | StorageType::Spinand => {
                    Some(("BOOT    ", "BOOT0_0000000000"))
                }
                StorageType::Sdcard | StorageType::Emmc | StorageType::Emmc3 | StorageType::Emmc0 => {
                    Some(("12345678", "1234567890BOOT_0"))
                }
                StorageType::Spinor => Some(("12345678", "1234567890BNOR_0")),
                StorageType::Ufs => Some(("12345678", "1234567890BUFS_0")),
                _ => Some(("12345678", "1234567890BOOT_0")),
            }
        } else {
            match StorageType::from(storage_type) {
                StorageType::Sdcard | StorageType::Sd1 => {
                    Some(("12345678", "TOC0_SDCARD00000"))
                }
                StorageType::Nand | StorageType::Spinand => {
                    Some(("12345678", "TOC0_NAND0000000"))
                }
                StorageType::Spinor => Some(("12345678", "TOC0_SPINOR00000")),
                StorageType::Ufs => Some(("12345678", "TOC0_UFS00000000")),
                _ => Some(("12345678", "TOC0_00000000000")),
            }
        }
    }
}
