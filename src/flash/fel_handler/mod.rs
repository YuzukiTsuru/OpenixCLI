mod dram_init;
mod uboot_download;

pub use dram_init::DramInit;
pub use uboot_download::UbootDownload;

use crate::utils::Logger;

pub struct FelHandler<'a> {
    logger: &'a Logger,
}

impl<'a> FelHandler<'a> {
    pub fn new(logger: &'a Logger) -> Self {
        Self { logger }
    }

    pub async fn handle(
        &self,
        ctx: &mut libefex::Context,
        fes_data: &[u8],
    ) -> crate::utils::FlashResult<()> {
        let dram_init = DramInit::new(self.logger);
        dram_init.execute(ctx, fes_data).await
    }

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
