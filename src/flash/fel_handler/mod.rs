//! FEL (Fastboot Entry Level) mode handler
//!
//! Handles FEL mode operations for devices in USB boot mode
//! FEL mode is used for initial device communication and DRAM initialization

mod dram_init;
mod uboot_download;

pub use dram_init::DramInit;
pub use uboot_download::UbootDownload;

use crate::flash::protocol::FelOps;
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
    pub async fn handle<C: FelOps>(
        &self,
        ctx: &mut C,
        fes_data: &[u8],
    ) -> crate::utils::FlashResult<()> {
        let dram_init = DramInit::new(self.logger);
        dram_init.execute(ctx, fes_data).await
    }

    /// Download U-Boot to device
    ///
    /// Transfers U-Boot image along with DTB, sys_config, and board_config
    pub async fn download_uboot<C: FelOps>(
        &self,
        ctx: &C,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::boot_header::{Boot0Header, UBootHeader};
    use crate::config::sys_config::DramParamInfo;
    use crate::flash::protocol::tests::MockProtocol;

    #[tokio::test]
    async fn wrapper_delegates_dram_and_uboot_operations() {
        let logger = Logger::for_events(false, crate::flash::FlashEventSink::none());
        let handler = FelHandler::new(&logger);
        let mut ctx = MockProtocol::default();

        let mut fes = vec![0; std::mem::size_of::<Boot0Header>()];
        let header = Boot0Header::parse_mut(&mut fes).unwrap();
        header.run_addr = 0x1000;
        header.ret_addr = 0x2000;
        let mut dram = DramParamInfo::create_empty();
        dram.dram_init_flag = 2;
        ctx.fel_reads.borrow_mut().push_back(Ok(dram.serialize()));
        handler.handle(&mut ctx, &fes).await.unwrap();

        let mut uboot = vec![0; std::mem::size_of::<UBootHeader>()];
        UBootHeader::parse_mut(&mut uboot)
            .unwrap()
            .uboot_head
            .run_addr = 0x4000;
        handler
            .download_uboot(&ctx, &uboot, None, b"sys", None)
            .await
            .unwrap();
        assert_eq!(&*ctx.fel_execs.borrow(), &[0x1000, 0x4000]);
    }
}
