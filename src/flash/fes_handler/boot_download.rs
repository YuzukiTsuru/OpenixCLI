//! Boot image download handler
//!
//! Handles downloading Preboot, Boot0, and Boot1 images to device storage

use crate::config::boot_header::{BOOT_FILE_MODE_NORMAL, BOOT_FILE_MODE_PKG, BOOT_FILE_MODE_TOC};
use crate::config::mbr_parser::EFEX_CRC32_VALID_FLAG;
use crate::firmware::{OpenixPacker, PackerError, StorageType};
use crate::flash::fes_handler::types::fes_data_type;
use crate::flash::protocol::FesOps;
use crate::utils::{FlashError, FlashResult, Logger};
use libefex::FesDataType;

/// Boot image download handler
///
/// Downloads Preboot, Boot0, and Boot1 images to device storage based on
/// the boot mode and storage type
pub struct BootDownload<'a> {
    logger: &'a Logger,
}

impl<'a> BootDownload<'a> {
    /// Create a new boot download handler
    pub fn new(logger: &'a Logger) -> Self {
        Self { logger }
    }

    /// Execute boot image download
    ///
    /// Downloads Preboot, Boot1, and Boot0 images to device
    pub async fn execute<C: FesOps>(
        &self,
        ctx: &C,
        packer: &mut OpenixPacker,
        secure: u32,
        storage_type: u32,
    ) -> FlashResult<()> {
        self.logger.info("Downloading Preboot/Boot0/Boot1...");

        self.download_preboot(ctx, packer, secure).await?;
        self.download_boot1(ctx, packer, secure, storage_type)
            .await?;
        self.download_boot0(ctx, packer, secure, storage_type)
            .await?;

        self.logger.stage_complete("Preboot/Boot0/Boot1 downloaded");
        Ok(())
    }

    /// Download the optional preboot image before Boot1.
    async fn download_preboot<C: FesOps>(
        &self,
        ctx: &C,
        packer: &mut OpenixPacker,
        secure: u32,
    ) -> FlashResult<()> {
        let Some(subtype) = self.get_preboot_subtype(secure) else {
            return Ok(());
        };

        self.logger
            .debug(&format!("Looking for Preboot: {}", subtype));

        if packer.find_file_header_by_subtype(subtype).is_none() {
            self.logger
                .debug(&format!("Preboot not found: {}, skipping", subtype));
            return Ok(());
        }

        let preboot_data = packer.find_file_data_by_subtype(subtype)?;
        self.logger.info(&format!(
            "Downloading Preboot: {} ({} bytes)",
            subtype,
            preboot_data.len()
        ));

        ctx.fes_down(&preboot_data, 0, FesDataType::Preboot)
            .map_err(|e| FlashError::UsbTransferError(e.to_string()))?;

        self.verify_boot(ctx, fes_data_type::PREBOOT, "Preboot")
            .await
    }

    /// Download Boot1 image
    ///
    /// Boot1 is the secondary boot loader
    async fn download_boot1<C: FesOps>(
        &self,
        ctx: &C,
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

                    self.verify_boot(ctx, fes_data_type::BOOT1, "Boot1").await?;
                }
                Err(e) => {
                    self.logger.debug(&format!(
                        "Boot1 not found: {}/{} - {}",
                        maintype, subtype, e
                    ));
                    return Err(FlashError::Boot1NotFound);
                }
            }
        }
        Ok(())
    }

    /// Download Boot0 image
    ///
    /// Boot0 is the primary boot loader stored in storage
    async fn download_boot0<C: FesOps>(
        &self,
        ctx: &C,
        packer: &mut OpenixPacker,
        secure: u32,
        storage_type: u32,
    ) -> FlashResult<()> {
        if let Some((maintype, subtype)) = self.get_boot0_entry(secure, storage_type) {
            self.logger
                .debug(&format!("Looking for Boot0: {maintype}/{subtype}"));

            let boot0_data = packer
                .get_file_data_by_maintype_subtype(maintype, subtype)
                .or_else(|_| {
                    self.get_boot0_fallback_entry(secure)
                        .filter(|fallback| *fallback != (maintype, subtype))
                        .map_or_else(
                            || Err(PackerError::FileNotFound(format!("{maintype}/{subtype}"))),
                            |(fallback_maintype, fallback_subtype)| {
                                packer.get_file_data_by_maintype_subtype(
                                    fallback_maintype,
                                    fallback_subtype,
                                )
                            },
                        )
                });

            match boot0_data {
                Ok(boot0_data) => {
                    self.logger.info(&format!(
                        "Downloading Boot0: {maintype}/{subtype} ({} bytes)",
                        boot0_data.len()
                    ));

                    ctx.fes_down(&boot0_data, 0, FesDataType::Boot0)
                        .map_err(|e| FlashError::UsbTransferError(e.to_string()))?;

                    self.verify_boot(ctx, fes_data_type::BOOT0, "Boot0").await?;
                }
                Err(e) => {
                    self.logger
                        .debug(&format!("Boot0 not found: {maintype}/{subtype} - {e}"));
                    return Err(FlashError::Boot0NotFound);
                }
            }
        }
        Ok(())
    }

    /// Verify boot image download
    async fn verify_boot<C: FesOps>(&self, ctx: &C, data_type: u32, name: &str) -> FlashResult<()> {
        let verify = ctx
            .fes_verify_status(data_type)
            .map_err(|e| FlashError::UsbTransferError(e.to_string()))?;

        if verify.flag == EFEX_CRC32_VALID_FLAG {
            self.logger.stage_complete(&format!("{} verified", name));
        } else {
            self.logger
                .warn(&format!("{} verify status: 0x{:04x}", name, verify.flag));
        }

        Ok(())
    }

    /// Get Preboot subtype based on boot mode.
    fn get_preboot_subtype(&self, secure: u32) -> Option<&'static str> {
        match secure {
            BOOT_FILE_MODE_NORMAL | BOOT_FILE_MODE_PKG => Some("1234567890PREB_0"),
            BOOT_FILE_MODE_TOC => Some("TOC0_PREBOOT0000"),
            _ => None,
        }
    }

    /// Get Boot1 subtype based on boot mode and storage type
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

    /// Get the Boot0 entry based on boot mode and storage type.
    fn get_boot0_entry(
        &self,
        secure: u32,
        storage_type: u32,
    ) -> Option<(&'static str, &'static str)> {
        if secure == BOOT_FILE_MODE_NORMAL || secure == BOOT_FILE_MODE_PKG {
            match StorageType::from(storage_type) {
                StorageType::Nand | StorageType::Spinand => Some(("BOOT", "BOOT0_0000000000")),
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

    fn get_boot0_fallback_entry(&self, secure: u32) -> Option<(&'static str, &'static str)> {
        match secure {
            BOOT_FILE_MODE_NORMAL | BOOT_FILE_MODE_PKG => Some(("12345678", "1234567890BOOT_0")),
            BOOT_FILE_MODE_TOC => Some(("12345678", "TOC0_00000000000")),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flash::protocol::tests::MockProtocol;
    use crate::test_support::{image_cfg_entries, test_firmware, FirmwareEntry};

    #[test]
    fn selects_preboot_item_for_each_supported_boot_mode() {
        let logger = Logger::new();
        let downloader = BootDownload::new(&logger);

        assert_eq!(
            downloader.get_preboot_subtype(BOOT_FILE_MODE_NORMAL),
            Some("1234567890PREB_0")
        );
        assert_eq!(
            downloader.get_preboot_subtype(BOOT_FILE_MODE_PKG),
            Some("1234567890PREB_0")
        );
        assert_eq!(
            downloader.get_preboot_subtype(BOOT_FILE_MODE_TOC),
            Some("TOC0_PREBOOT0000")
        );
        assert_eq!(downloader.get_preboot_subtype(u32::MAX), None);
    }

    #[test]
    fn preboot_verify_type_matches_efex_protocol() {
        assert_eq!(fes_data_type::PREBOOT, 0x7f08);
    }

    #[test]
    fn boot_item_selection_covers_modes_and_storage_types() {
        let logger = Logger::new();
        let downloader = BootDownload::new(&logger);
        assert_eq!(
            downloader.get_boot1_subtype(BOOT_FILE_MODE_NORMAL, 8),
            Some(("12345678", "UBOOT_0000000000"))
        );
        assert_eq!(
            downloader.get_boot1_subtype(BOOT_FILE_MODE_TOC, 8),
            Some(("12345678", "TOC1_00000000000"))
        );
        assert_eq!(
            downloader.get_boot1_subtype(BOOT_FILE_MODE_PKG, StorageType::Spinor as u32),
            Some(("12345678", "BOOTPKG-NOR00000"))
        );
        assert_eq!(
            downloader.get_boot1_subtype(BOOT_FILE_MODE_PKG, StorageType::Ufs as u32),
            Some(("12345678", "BOOTPKG-00000000"))
        );
        assert_eq!(downloader.get_boot1_subtype(u32::MAX, 0), None);

        let normal_cases = [
            (StorageType::Nand, ("BOOT", "BOOT0_0000000000")),
            (StorageType::Spinand, ("BOOT", "BOOT0_0000000000")),
            (StorageType::Sdcard, ("12345678", "1234567890BOOT_0")),
            (StorageType::Emmc, ("12345678", "1234567890BOOT_0")),
            (StorageType::Emmc3, ("12345678", "1234567890BOOT_0")),
            (StorageType::Emmc0, ("12345678", "1234567890BOOT_0")),
            (StorageType::Spinor, ("12345678", "1234567890BNOR_0")),
            (StorageType::Ufs, ("12345678", "1234567890BUFS_0")),
            (StorageType::Auto, ("12345678", "1234567890BOOT_0")),
        ];
        for (storage, expected) in normal_cases {
            assert_eq!(
                downloader.get_boot0_entry(BOOT_FILE_MODE_NORMAL, storage as u32),
                Some(expected)
            );
        }
        let toc_cases = [
            (StorageType::Sdcard, ("12345678", "TOC0_SDCARD00000")),
            (StorageType::Sd1, ("12345678", "TOC0_SDCARD00000")),
            (StorageType::Nand, ("12345678", "TOC0_NAND0000000")),
            (StorageType::Spinand, ("12345678", "TOC0_NAND0000000")),
            (StorageType::Spinor, ("12345678", "TOC0_SPINOR00000")),
            (StorageType::Ufs, ("12345678", "TOC0_UFS00000000")),
            (StorageType::Auto, ("12345678", "TOC0_00000000000")),
        ];
        for (storage, expected) in toc_cases {
            assert_eq!(
                downloader.get_boot0_entry(BOOT_FILE_MODE_TOC, storage as u32),
                Some(expected)
            );
        }
        assert_eq!(
            downloader.get_boot0_fallback_entry(BOOT_FILE_MODE_NORMAL),
            Some(("12345678", "1234567890BOOT_0"))
        );
        assert_eq!(
            downloader.get_boot0_fallback_entry(BOOT_FILE_MODE_TOC),
            Some(("12345678", "TOC0_00000000000"))
        );
        assert_eq!(downloader.get_boot0_fallback_entry(u32::MAX), None);
    }

    #[tokio::test]
    async fn toc_execute_sends_preboot_boot1_boot0_in_order_and_verifies_each() {
        let firmware = test_firmware(&[
            FirmwareEntry {
                filename: "preboot.fex",
                maintype: "12345678",
                subtype: "TOC0_PREBOOT0000",
                data: b"preboot",
            },
            FirmwareEntry {
                filename: "toc1.fex",
                maintype: "12345678",
                subtype: "TOC1_00000000000",
                data: b"boot1",
            },
            FirmwareEntry {
                filename: "toc0.fex",
                maintype: "12345678",
                subtype: "TOC0_UFS00000000",
                data: b"boot0",
            },
        ]);
        let mut packer = OpenixPacker::new();
        packer.load(firmware.path()).unwrap();
        let logger = Logger::for_events(false, crate::flash::FlashEventSink::none());
        let downloader = BootDownload::new(&logger);
        let ctx = MockProtocol::default();

        downloader
            .execute(
                &ctx,
                &mut packer,
                BOOT_FILE_MODE_TOC,
                StorageType::Ufs as u32,
            )
            .await
            .unwrap();

        let downloads = ctx.downloads.borrow();
        assert_eq!(downloads.len(), 3);
        assert_eq!(downloads[0].data_type, FesDataType::Preboot);
        assert_eq!(downloads[1].data_type, FesDataType::Boot1);
        assert_eq!(downloads[2].data_type, FesDataType::Boot0);
        assert_eq!(
            &*ctx.verify_status_calls.borrow(),
            &[
                fes_data_type::PREBOOT,
                fes_data_type::BOOT1,
                fes_data_type::BOOT0
            ]
        );
    }

    #[tokio::test]
    async fn preboot_is_optional_and_generic_boot0_is_a_real_fallback() {
        let firmware = test_firmware(&[
            FirmwareEntry {
                filename: "uboot.fex",
                maintype: "12345678",
                subtype: "UBOOT_0000000000",
                data: b"boot1",
            },
            FirmwareEntry {
                filename: "boot0.fex",
                maintype: "12345678",
                subtype: "1234567890BOOT_0",
                data: b"generic",
            },
        ]);
        let mut packer = OpenixPacker::new();
        packer.load(firmware.path()).unwrap();
        let logger = Logger::for_events(false, crate::flash::FlashEventSink::none());
        let downloader = BootDownload::new(&logger);
        let ctx = MockProtocol::default();
        downloader
            .execute(
                &ctx,
                &mut packer,
                BOOT_FILE_MODE_NORMAL,
                StorageType::Ufs as u32,
            )
            .await
            .unwrap();
        let downloads = ctx.downloads.borrow();
        assert_eq!(downloads.len(), 2);
        assert_eq!(downloads[1].data, b"generic");
    }

    #[tokio::test]
    async fn package_mode_uses_the_boot_package_mapping_from_image_cfg() {
        let config = image_cfg_entries();
        let bootpkg = config
            .iter()
            .find(|entry| entry.filename == "boot_package.fex")
            .unwrap();
        let boot0 = config
            .iter()
            .find(|entry| entry.filename == "boot0_sdcard.fex")
            .unwrap();
        let firmware = test_firmware(&[
            FirmwareEntry {
                filename: &bootpkg.filename,
                maintype: &bootpkg.maintype,
                subtype: &bootpkg.subtype,
                data: b"boot1",
            },
            FirmwareEntry {
                filename: &boot0.filename,
                maintype: &boot0.maintype,
                subtype: &boot0.subtype,
                data: b"boot0",
            },
        ]);
        let mut packer = OpenixPacker::new();
        packer.load(firmware.path()).unwrap();
        let logger = Logger::for_events(false, crate::flash::FlashEventSink::none());
        let ctx = MockProtocol::default();
        BootDownload::new(&logger)
            .execute(
                &ctx,
                &mut packer,
                BOOT_FILE_MODE_PKG,
                StorageType::Emmc as u32,
            )
            .await
            .unwrap();
        assert_eq!(ctx.downloads.borrow()[0].data_type, FesDataType::Boot1);
    }

    #[tokio::test]
    async fn nand_mode_accepts_the_boot_maintype_from_image_cfg() {
        let config = image_cfg_entries();
        let uboot = config
            .iter()
            .find(|entry| entry.filename == "u-boot-efex.fex")
            .unwrap();
        let boot0 = config
            .iter()
            .find(|entry| entry.filename == "boot0_nand.fex")
            .unwrap();
        assert_eq!(boot0.maintype, "BOOT");

        let firmware = test_firmware(&[
            FirmwareEntry {
                filename: &uboot.filename,
                maintype: &uboot.maintype,
                subtype: &uboot.subtype,
                data: b"boot1",
            },
            FirmwareEntry {
                filename: &boot0.filename,
                maintype: &boot0.maintype,
                subtype: &boot0.subtype,
                data: b"nand boot0",
            },
        ]);
        let mut packer = OpenixPacker::new();
        packer.load(firmware.path()).unwrap();
        let logger = Logger::for_events(false, crate::flash::FlashEventSink::none());
        let ctx = MockProtocol::default();

        BootDownload::new(&logger)
            .execute(
                &ctx,
                &mut packer,
                BOOT_FILE_MODE_NORMAL,
                StorageType::Nand as u32,
            )
            .await
            .unwrap();

        let downloads = ctx.downloads.borrow();
        assert_eq!(downloads.len(), 2);
        assert_eq!(downloads[1].data_type, FesDataType::Boot0);
        assert_eq!(downloads[1].data, b"nand boot0");
    }

    #[tokio::test]
    async fn missing_required_boot_items_and_protocol_errors_are_reported() {
        let logger = Logger::for_events(false, crate::flash::FlashEventSink::none());
        let downloader = BootDownload::new(&logger);
        let ctx = MockProtocol::default();

        let empty = test_firmware(&[]);
        let mut packer = OpenixPacker::new();
        packer.load(empty.path()).unwrap();
        assert!(matches!(
            downloader
                .execute(
                    &ctx,
                    &mut packer,
                    BOOT_FILE_MODE_NORMAL,
                    StorageType::Ufs as u32
                )
                .await,
            Err(FlashError::Boot1NotFound)
        ));

        let boot1_only = test_firmware(&[FirmwareEntry {
            filename: "uboot.fex",
            maintype: "12345678",
            subtype: "UBOOT_0000000000",
            data: b"boot1",
        }]);
        let mut packer = OpenixPacker::new();
        packer.load(boot1_only.path()).unwrap();
        assert!(matches!(
            downloader
                .execute(
                    &ctx,
                    &mut packer,
                    BOOT_FILE_MODE_NORMAL,
                    StorageType::Ufs as u32
                )
                .await,
            Err(FlashError::Boot0NotFound)
        ));

        let complete = test_firmware(&[
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
        packer.load(complete.path()).unwrap();
        let failed_down = MockProtocol::default();
        *failed_down.fail_down.borrow_mut() = Some("down".to_string());
        assert!(matches!(
            downloader
                .execute(
                    &failed_down,
                    &mut packer,
                    BOOT_FILE_MODE_NORMAL,
                    StorageType::Ufs as u32
                )
                .await,
            Err(FlashError::UsbTransferError(_))
        ));

        let mut packer = OpenixPacker::new();
        packer.load(complete.path()).unwrap();
        let failed_verify = MockProtocol::default();
        failed_verify
            .verify_statuses
            .borrow_mut()
            .push_back(Err("verify".to_string()));
        assert!(matches!(
            downloader
                .execute(
                    &failed_verify,
                    &mut packer,
                    BOOT_FILE_MODE_NORMAL,
                    StorageType::Ufs as u32
                )
                .await,
            Err(FlashError::UsbTransferError(_))
        ));
    }
}
