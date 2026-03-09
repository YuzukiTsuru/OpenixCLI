use crate::config::boot_header::{BOOT_FILE_MODE_NORMAL, BOOT_FILE_MODE_PKG, BOOT_FILE_MODE_TOC};
use crate::config::mbr_parser::EFEX_CRC32_VALID_FLAG;
use crate::firmware::{OpenixPacker, PackerError, StorageType};
use crate::utils::{FlashError, FlashResult, Logger};
use libefex::FesDataType;

pub struct BootDownload<'a> {
    logger: &'a Logger,
}

impl<'a> BootDownload<'a> {
    pub fn new(logger: &'a Logger) -> Self {
        Self { logger }
    }

    pub async fn execute(
        &self,
        ctx: &libefex::Context,
        packer: &mut OpenixPacker,
        secure: u32,
        storage_type: u32,
    ) -> FlashResult<()> {
        self.logger.info("Downloading Boot0/Boot1...");

        self.download_boot1(ctx, packer, secure, storage_type)
            .await?;
        self.download_boot0(ctx, packer, secure, storage_type)
            .await?;

        self.logger.stage_complete("Boot0/Boot1 downloaded");
        Ok(())
    }

    async fn download_boot1(
        &self,
        ctx: &libefex::Context,
        packer: &mut OpenixPacker,
        secure: u32,
        storage_type: u32,
    ) -> FlashResult<()> {
        if let Some((maintype, subtype)) = self.get_boot1_subtype(secure, storage_type) {
            self.logger
                .debug(&format!("Looking for Boot1: {}/{}", maintype, subtype));
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
        Ok(())
    }

    async fn download_boot0(
        &self,
        ctx: &libefex::Context,
        packer: &mut OpenixPacker,
        secure: u32,
        storage_type: u32,
    ) -> FlashResult<()> {
        if let Some((maintype, subtype)) = self.get_boot0_subtype(secure, storage_type) {
            self.logger
                .debug(&format!("Looking for Boot0: {}/{}", maintype, subtype));
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
        Ok(())
    }

    fn get_boot1_subtype(
        &self,
        secure: u32,
        storage_type: u32,
    ) -> Option<(&'static str, &'static str)> {
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

    fn get_boot0_subtype(
        &self,
        secure: u32,
        storage_type: u32,
    ) -> Option<(&'static str, &'static str)> {
        if secure == BOOT_FILE_MODE_NORMAL || secure == BOOT_FILE_MODE_PKG {
            match StorageType::from(storage_type) {
                StorageType::Nand | StorageType::Spinand => Some(("BOOT    ", "BOOT0_0000000000")),
                StorageType::Sdcard
                | StorageType::Emmc
                | StorageType::Emmc3
                | StorageType::Emmc0 => Some(("12345678", "1234567890BOOT_0")),
                StorageType::Spinor => Some(("12345678", "1234567890BNOR_0")),
                StorageType::Ufs => Some(("12345678", "1234567890BUFS_0")),
                _ => Some(("12345678", "1234567890BOOT_0")),
            }
        } else {
            match StorageType::from(storage_type) {
                StorageType::Sdcard | StorageType::Sd1 => Some(("12345678", "TOC0_SDCARD00000")),
                StorageType::Nand | StorageType::Spinand => Some(("12345678", "TOC0_NAND0000000")),
                StorageType::Spinor => Some(("12345678", "TOC0_SPINOR00000")),
                StorageType::Ufs => Some(("12345678", "TOC0_UFS00000000")),
                _ => Some(("12345678", "TOC0_00000000000")),
            }
        }
    }
}
