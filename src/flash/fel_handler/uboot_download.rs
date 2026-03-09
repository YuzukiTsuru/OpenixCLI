use crate::config::boot_header::{UBootHeader, WORK_MODE_USB_PRODUCT};
use crate::utils::{FlashError, FlashResult, Logger};

const UBOOT_MAX_LEN: usize = 2 * 1024 * 1024;
const DTB_MAX_LEN: usize = 1024 * 1024;
const SYS_CONFIG_BIN00_MAX_LEN: usize = 512 * 1024;

pub struct UbootDownload<'a> {
    logger: &'a Logger,
}

impl<'a> UbootDownload<'a> {
    pub fn new(logger: &'a Logger) -> Self {
        Self { logger }
    }

    pub async fn execute(
        &self,
        ctx: &libefex::Context,
        uboot_data: &[u8],
        dtb_data: Option<&[u8]>,
        sysconfig_data: &[u8],
        board_config_data: Option<&[u8]>,
    ) -> FlashResult<()> {
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

    fn write_dtb(
        &self,
        ctx: &libefex::Context,
        run_addr: u32,
        dtb_data: Option<&[u8]>,
    ) -> FlashResult<()> {
        if let Some(dtb) = dtb_data {
            let dtb_sysconfig_base = run_addr + UBOOT_MAX_LEN as u32;
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

    fn write_sysconfig(
        &self,
        ctx: &libefex::Context,
        run_addr: u32,
        sysconfig_data: &[u8],
    ) -> FlashResult<()> {
        let dtb_sysconfig_base = run_addr + UBOOT_MAX_LEN as u32;
        let sys_config_bin_base = dtb_sysconfig_base + DTB_MAX_LEN as u32;
        ctx.fel_write(sys_config_bin_base, sysconfig_data)
            .map_err(|e| FlashError::UsbTransferError(e.to_string()))?;
        self.logger.debug(&format!(
            "SysConfig written to 0x{:x} ({} bytes)",
            sys_config_bin_base,
            sysconfig_data.len()
        ));
        Ok(())
    }

    fn write_board_config(
        &self,
        ctx: &libefex::Context,
        run_addr: u32,
        board_config_data: Option<&[u8]>,
    ) -> FlashResult<()> {
        if let Some(board_config) = board_config_data {
            let dtb_sysconfig_base = run_addr + UBOOT_MAX_LEN as u32;
            let sys_config_bin_base = dtb_sysconfig_base + DTB_MAX_LEN as u32;
            let board_config_bin_base = sys_config_bin_base + SYS_CONFIG_BIN00_MAX_LEN as u32;
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
}
