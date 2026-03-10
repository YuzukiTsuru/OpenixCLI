//! FEL (Fastboot Entry Level) mode handler
//!
//! Handles FEL mode operations for devices in USB boot mode
//! FEL mode is used for initial device communication and DRAM initialization

mod dram_init;
mod uboot_download;

pub use dram_init::DramInit;
pub use uboot_download::UbootDownload;

use crate::utils::Logger;

/// FEL handler for devices in USB boot mode
///
/// Handles DRAM initialization and U-Boot download for devices
/// that are in FEL mode (USB boot)
pub struct FelHandler<'a> {
    logger: &'a Logger,
}

impl<'a> FelHandler<'a> {
    /// Create a new FEL handler
    pub fn new(logger: &'a Logger) -> Self {
        Self { logger }
    }

    /// Handle FEL mode operations
    ///
    /// Initializes DRAM and prepares device for flashing
    pub async fn handle(
        &self,
        ctx: &mut libefex::Context,
        fes_data: &[u8],
    ) -> crate::utils::FlashResult<()> {
        let dram_init = DramInit::new(self.logger);
        dram_init.execute(ctx, fes_data).await
    }

    /// Download U-Boot to device
    ///
    /// Transfers U-Boot image along with DTB, sys_config, and board_config
    pub async fn download_uboot(
        &self,
        ctx: &libefex::Context,
        uboot_data: &[u8],
        dtb_data: Option<&[u8]>,
        sysconfig_data: &[u8],
        board_config_data: Option<&[u8]>,
    ) -> crate::utils::FlashResult<()> {
        let uboot_download = UbootDownload::new(self.logger);
        uboot_download
            .execute(ctx, uboot_data, dtb_data, sysconfig_data, board_config_data)
            .await
    }
}
