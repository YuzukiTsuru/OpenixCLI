//! U-Boot download handler
//!
//! Handles downloading U-Boot and related configurations to device memory

use crate::config::boot_header::{UBootHeader, WORK_MODE_USB_PRODUCT};
use crate::flash::protocol::FelOps;
use crate::utils::{FlashError, FlashResult, Logger};

/// Maximum U-Boot size (2 MB)
const UBOOT_MAX_LEN: usize = 2 * 1024 * 1024;
/// Maximum DTB size (1 MB)
const DTB_MAX_LEN: usize = 1024 * 1024;
/// Maximum sys_config.bin size (512 KB)
const SYS_CONFIG_BIN00_MAX_LEN: usize = 512 * 1024;
/// Maximum board_config.bin size (512 KB)
const BOARD_CONFIG_BIN_MAX_LEN: usize = 512 * 1024;

/// U-Boot download handler
///
/// Downloads U-Boot image, DTB, sys_config, and board_config to device memory
pub struct UbootDownload<'a> {
    logger: &'a Logger,
}

impl<'a> UbootDownload<'a> {
    /// Create a new U-Boot download handler
    pub fn new(logger: &'a Logger) -> Self {
        Self { logger }
    }

    /// Execute U-Boot download
    ///
    /// Downloads U-Boot image with work mode set to USB product mode,
    /// then downloads DTB, sys_config, and board_config to appropriate memory locations
    pub async fn execute<C: FelOps>(
        &self,
        ctx: &C,
        uboot_data: &[u8],
        dtb_data: Option<&[u8]>,
        sysconfig_data: &[u8],
        board_config_data: Option<&[u8]>,
    ) -> FlashResult<()> {
        if uboot_data.len() > UBOOT_MAX_LEN {
            return Err(FlashError::InvalidFirmwareFormat(format!(
                "U-Boot exceeds {} byte memory slot",
                UBOOT_MAX_LEN
            )));
        }
        if dtb_data.is_some_and(|data| data.len() > DTB_MAX_LEN) {
            return Err(FlashError::InvalidFirmwareFormat(format!(
                "DTB exceeds {} byte memory slot",
                DTB_MAX_LEN
            )));
        }
        if sysconfig_data.len() > SYS_CONFIG_BIN00_MAX_LEN {
            return Err(FlashError::InvalidFirmwareFormat(format!(
                "SysConfig exceeds {} byte memory slot",
                SYS_CONFIG_BIN00_MAX_LEN
            )));
        }
        if board_config_data.is_some_and(|data| data.len() > BOARD_CONFIG_BIN_MAX_LEN) {
            return Err(FlashError::InvalidFirmwareFormat(format!(
                "BoardConfig exceeds {} byte memory slot",
                BOARD_CONFIG_BIN_MAX_LEN
            )));
        }

        self.logger.info(&format!(
            "Downloading U-Boot ({} bytes)...",
            uboot_data.len()
        ));

        let mut uboot_buffer = uboot_data.to_vec();
        UBootHeader::set_work_mode(&mut uboot_buffer, WORK_MODE_USB_PRODUCT);

        let uboot_head = UBootHeader::parse(&uboot_buffer)
            .map_err(|e| FlashError::InvalidFirmwareFormat(e.to_string()))?;

        let run_addr = uboot_head.uboot_head.run_addr;

        self.logger.debug(&format!(
            "U-Boot magic: {}, addr: 0x{:x}",
            uboot_head.uboot_head.magic_str(),
            run_addr
        ));

        let timeout_secs = std::cmp::max(10, uboot_data.len() / (64 * 1024));
        self.logger.debug(&format!(
            "Setting timeout to {}s for {} bytes",
            timeout_secs,
            uboot_data.len()
        ));

        ctx.fel_write(run_addr, &uboot_buffer)
            .map_err(|e| FlashError::UsbTransferError(e.to_string()))?;

        self.write_dtb(ctx, run_addr, dtb_data)?;
        self.write_sysconfig(ctx, run_addr, sysconfig_data)?;
        self.write_board_config(ctx, run_addr, board_config_data)?;

        self.logger
            .debug(&format!("Executing U-Boot at 0x{:x}", run_addr));
        ctx.fel_exec(run_addr)
            .map_err(|e| FlashError::UsbTransferError(e.to_string()))?;

        self.logger.info("U-Boot downloaded and executed");
        Ok(())
    }

    /// Write DTB (Device Tree Blob) to device memory
    ///
    /// DTB is placed after U-Boot in memory
    fn write_dtb<C: FelOps>(
        &self,
        ctx: &C,
        run_addr: u32,
        dtb_data: Option<&[u8]>,
    ) -> FlashResult<()> {
        if let Some(dtb) = dtb_data {
            let dtb_sysconfig_base = Self::checked_address(run_addr, UBOOT_MAX_LEN)?;
            ctx.fel_write(dtb_sysconfig_base, dtb)
                .map_err(|e| FlashError::UsbTransferError(e.to_string()))?;
            self.logger.debug(&format!(
                "DTB written to 0x{:x} ({} bytes)",
                dtb_sysconfig_base,
                dtb.len()
            ));
        }
        Ok(())
    }

    /// Write system configuration to device memory
    ///
    /// SysConfig is placed after DTB in memory
    fn write_sysconfig<C: FelOps>(
        &self,
        ctx: &C,
        run_addr: u32,
        sysconfig_data: &[u8],
    ) -> FlashResult<()> {
        let sys_config_bin_base = Self::checked_address(run_addr, UBOOT_MAX_LEN + DTB_MAX_LEN)?;
        ctx.fel_write(sys_config_bin_base, sysconfig_data)
            .map_err(|e| FlashError::UsbTransferError(e.to_string()))?;
        self.logger.debug(&format!(
            "SysConfig written to 0x{:x} ({} bytes)",
            sys_config_bin_base,
            sysconfig_data.len()
        ));
        Ok(())
    }

    /// Write board configuration to device memory
    ///
    /// BoardConfig is placed after sys_config in memory
    fn write_board_config<C: FelOps>(
        &self,
        ctx: &C,
        run_addr: u32,
        board_config_data: Option<&[u8]>,
    ) -> FlashResult<()> {
        if let Some(board_config) = board_config_data {
            let board_config_bin_base = Self::checked_address(
                run_addr,
                UBOOT_MAX_LEN + DTB_MAX_LEN + SYS_CONFIG_BIN00_MAX_LEN,
            )?;
            ctx.fel_write(board_config_bin_base, board_config)
                .map_err(|e| FlashError::UsbTransferError(e.to_string()))?;
            self.logger.debug(&format!(
                "BoardConfig written to 0x{:x} ({} bytes)",
                board_config_bin_base,
                board_config.len()
            ));
        }
        Ok(())
    }

    fn checked_address(run_addr: u32, offset: usize) -> FlashResult<u32> {
        run_addr.checked_add(offset as u32).ok_or_else(|| {
            FlashError::InvalidFirmwareFormat(format!(
                "U-Boot component address overflow: 0x{run_addr:08x} + 0x{offset:x}"
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flash::protocol::tests::MockProtocol;

    fn uboot_image(run_addr: u32) -> Vec<u8> {
        let mut data = vec![0u8; std::mem::size_of::<UBootHeader>()];
        let header = UBootHeader::parse_mut(&mut data).unwrap();
        header.uboot_head.magic = *b"uboot\0\0\0";
        header.uboot_head.run_addr = run_addr;
        data
    }

    #[tokio::test]
    async fn execute_places_all_components_in_non_overlapping_slots() {
        let logger = Logger::for_events(false, crate::flash::FlashEventSink::none());
        let handler = UbootDownload::new(&logger);
        let ctx = MockProtocol::default();
        let run_addr = 0x4000_0000;

        handler
            .execute(
                &ctx,
                &uboot_image(run_addr),
                Some(b"dtb"),
                b"sys",
                Some(b"board"),
            )
            .await
            .unwrap();

        let writes = ctx.fel_writes.borrow();
        assert_eq!(writes.len(), 4);
        assert_eq!(writes[0].addr, run_addr);
        assert_eq!(writes[1].addr, run_addr + UBOOT_MAX_LEN as u32);
        assert_eq!(
            writes[2].addr,
            run_addr + (UBOOT_MAX_LEN + DTB_MAX_LEN) as u32
        );
        assert_eq!(
            writes[3].addr,
            run_addr + (UBOOT_MAX_LEN + DTB_MAX_LEN + SYS_CONFIG_BIN00_MAX_LEN) as u32
        );
        let uploaded = UBootHeader::parse(&writes[0].data).unwrap();
        let mode = uploaded.uboot_data.work_mode;
        assert_eq!(mode, WORK_MODE_USB_PRODUCT as i32);
        assert_eq!(&*ctx.fel_execs.borrow(), &[run_addr]);
    }

    #[tokio::test]
    async fn execute_skips_optional_components() {
        let logger = Logger::for_events(false, crate::flash::FlashEventSink::none());
        let handler = UbootDownload::new(&logger);
        let ctx = MockProtocol::default();
        handler
            .execute(&ctx, &uboot_image(0x1000), None, b"sys", None)
            .await
            .unwrap();
        assert_eq!(ctx.fel_writes.borrow().len(), 2);
    }

    #[tokio::test]
    async fn execute_rejects_short_and_oversized_components() {
        let logger = Logger::for_events(false, crate::flash::FlashEventSink::none());
        let handler = UbootDownload::new(&logger);
        let ctx = MockProtocol::default();

        assert!(matches!(
            handler.execute(&ctx, &[], None, &[], None).await,
            Err(FlashError::InvalidFirmwareFormat(_))
        ));
        assert!(matches!(
            handler
                .execute(&ctx, &vec![0; UBOOT_MAX_LEN + 1], None, &[], None)
                .await,
            Err(FlashError::InvalidFirmwareFormat(_))
        ));
        assert!(matches!(
            handler
                .execute(
                    &ctx,
                    &uboot_image(0),
                    Some(&vec![0; DTB_MAX_LEN + 1]),
                    &[],
                    None
                )
                .await,
            Err(FlashError::InvalidFirmwareFormat(_))
        ));
        assert!(matches!(
            handler
                .execute(
                    &ctx,
                    &uboot_image(0),
                    None,
                    &[],
                    Some(&vec![0; BOARD_CONFIG_BIN_MAX_LEN + 1])
                )
                .await,
            Err(FlashError::InvalidFirmwareFormat(_))
        ));
        assert!(matches!(
            handler
                .execute(
                    &ctx,
                    &uboot_image(0),
                    None,
                    &vec![0; SYS_CONFIG_BIN00_MAX_LEN + 1],
                    None
                )
                .await,
            Err(FlashError::InvalidFirmwareFormat(_))
        ));
    }

    #[test]
    fn component_writers_detect_address_overflow_and_transport_errors() {
        let logger = Logger::for_events(false, crate::flash::FlashEventSink::none());
        let handler = UbootDownload::new(&logger);
        let ctx = MockProtocol::default();
        assert!(matches!(
            handler.write_dtb(&ctx, u32::MAX, Some(b"dtb")),
            Err(FlashError::InvalidFirmwareFormat(_))
        ));
        assert!(matches!(
            handler.write_sysconfig(&ctx, u32::MAX, b"sys"),
            Err(FlashError::InvalidFirmwareFormat(_))
        ));
        assert!(matches!(
            handler.write_board_config(&ctx, u32::MAX, Some(b"board")),
            Err(FlashError::InvalidFirmwareFormat(_))
        ));

        *ctx.fail_fel_write.borrow_mut() = Some("write".to_string());
        assert!(matches!(
            handler.write_dtb(&ctx, 0, Some(b"dtb")),
            Err(FlashError::UsbTransferError(_))
        ));

        *ctx.fail_fel_write.borrow_mut() = Some("sysconfig write".to_string());
        assert!(matches!(
            handler.write_sysconfig(&ctx, 0, b"sys"),
            Err(FlashError::UsbTransferError(_))
        ));

        *ctx.fail_fel_write.borrow_mut() = Some("board config write".to_string());
        assert!(matches!(
            handler.write_board_config(&ctx, 0, Some(b"board")),
            Err(FlashError::UsbTransferError(_))
        ));
    }

    #[tokio::test]
    async fn execute_propagates_write_and_exec_errors() {
        let logger = Logger::for_events(false, crate::flash::FlashEventSink::none());
        let handler = UbootDownload::new(&logger);
        let ctx = MockProtocol::default();
        *ctx.fail_fel_write.borrow_mut() = Some("write".to_string());
        assert!(matches!(
            handler
                .execute(&ctx, &uboot_image(0), None, &[], None)
                .await,
            Err(FlashError::UsbTransferError(_))
        ));

        let ctx = MockProtocol::default();
        *ctx.fail_fel_exec.borrow_mut() = Some("exec".to_string());
        assert!(matches!(
            handler
                .execute(&ctx, &uboot_image(0), None, &[], None)
                .await,
            Err(FlashError::UsbTransferError(_))
        ));
    }
}
