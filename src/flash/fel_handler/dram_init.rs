use crate::config::boot_header::Boot0Header;
use crate::config::sys_config::DramParamInfo;
use crate::utils::{FlashError, FlashResult, Logger};
use std::time::Duration;

const DRAM_INIT_CHECK_INTERVAL: Duration = Duration::from_millis(1000);
const DRAM_INIT_TIMEOUT: Duration = Duration::from_secs(60);

pub struct DramInit<'a> {
    logger: &'a Logger,
}

impl<'a> DramInit<'a> {
    pub fn new(logger: &'a Logger) -> Self {
        Self { logger }
    }

    pub async fn execute(
        &self,
        ctx: &mut libefex::Context,
        fes_data: &[u8],
    ) -> FlashResult<()> {
        self.logger.info("Initializing DRAM...");

        let fes_head = Boot0Header::parse(fes_data)
            .map_err(|e| FlashError::InvalidFirmwareFormat(e.to_string()))?;

        let run_addr = fes_head.run_addr;
        let ret_addr = fes_head.ret_addr;

        self.logger.debug(&format!(
            "FES magic: {}, run_addr: 0x{:x}, ret_addr: 0x{:x}",
            fes_head.magic_str(),
            run_addr,
            ret_addr
        ));

        let dram_param = DramParamInfo::create_empty();
        let dram_buffer = dram_param.serialize();

        self.logger
            .debug(&format!("Clearing DRAM param area at 0x{:x}", ret_addr));
        ctx.fel_write(ret_addr, &dram_buffer)
            .map_err(|e| FlashError::UsbTransferError(e.to_string()))?;

        let timeout_secs = std::cmp::max(3, fes_data.len() / (64 * 1024));
        self.logger.debug(&format!(
            "Downloading {} bytes FES to device (timeout: {}s)...",
            fes_data.len(),
            timeout_secs
        ));

        ctx.fel_write(run_addr, fes_data)
            .map_err(|e| FlashError::UsbTransferError(e.to_string()))?;

        self.logger
            .debug(&format!("Executing FES at 0x{:x}", run_addr));
        ctx.fel_exec(run_addr)
            .map_err(|e| FlashError::UsbTransferError(e.to_string()))?;

        self.wait_for_dram_init(ctx, ret_addr).await?;

        self.logger.info("DRAM initialized successfully");
        Ok(())
    }

    async fn wait_for_dram_init(
        &self,
        ctx: &mut libefex::Context,
        ret_addr: u32,
    ) -> FlashResult<()> {
        self.logger.info("Waiting for DRAM initialization...");
        let start = std::time::Instant::now();
        let mut attempts = 0;
        let mut dram_info = DramParamInfo::create_empty();

        while start.elapsed() < DRAM_INIT_TIMEOUT {
            attempts += 1;
            tokio::time::sleep(DRAM_INIT_CHECK_INTERVAL).await;

            let mut dram_result = vec![0u8; std::mem::size_of::<DramParamInfo>()];
            match ctx.fel_read(ret_addr, &mut dram_result) {
                Ok(_) => {
                    dram_info = *DramParamInfo::parse(&dram_result)
                        .map_err(|e| FlashError::InvalidFirmwareFormat(e.to_string()))?;

                    let dram_init_flag = dram_info.dram_init_flag;
                    let dram_update_flag = dram_info.dram_update_flag;

                    self.logger.debug(&format!(
                        "DRAM init check #{}: init_flag={}, update_flag={}",
                        attempts, dram_init_flag, dram_update_flag
                    ));

                    if dram_init_flag != 0 {
                        break;
                    }
                }
                Err(e) => {
                    self.logger
                        .debug(&format!("DRAM init check #{} failed: {}", attempts, e));
                }
            }
        }

        let elapsed = start.elapsed();
        self.logger.debug(&format!(
            "DRAM init completed after {} attempts, {:?}",
            attempts, elapsed
        ));

        let dram_init_flag = dram_info.dram_init_flag;
        if dram_init_flag == 1 {
            return Err(FlashError::DramInitFailed);
        }

        Ok(())
    }
}
