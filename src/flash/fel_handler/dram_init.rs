//! DRAM initialization handler
//!
//! Handles DRAM initialization for Allwinner devices in FEL mode

use crate::config::boot_header::Boot0Header;
use crate::config::sys_config::DramParamInfo;
use crate::flash::protocol::FelOps;
use crate::utils::{FlashError, FlashResult, Logger};
use std::time::Duration;

/// Interval between DRAM initialization checks
const DRAM_INIT_CHECK_INTERVAL: Duration = Duration::from_millis(1000);
/// Maximum time to wait for DRAM initialization
const DRAM_INIT_TIMEOUT: Duration = Duration::from_secs(60);

fn dram_init_check_interval() -> Duration {
    if cfg!(test) {
        Duration::ZERO
    } else {
        DRAM_INIT_CHECK_INTERVAL
    }
}

/// DRAM initialization handler
pub struct DramInit<'a> {
    logger: &'a Logger,
}

impl<'a> DramInit<'a> {
    /// Create a new DRAM initialization handler
    pub fn new(logger: &'a Logger) -> Self {
        Self { logger }
    }

    /// Execute DRAM initialization
    ///
    /// Downloads FES (Flash Eraser Script) to device and initializes DRAM
    pub async fn execute<C: FelOps>(&self, ctx: &mut C, fes_data: &[u8]) -> FlashResult<()> {
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

        let max_attempts =
            (DRAM_INIT_TIMEOUT.as_millis() / DRAM_INIT_CHECK_INTERVAL.as_millis()).max(1) as usize;
        self.wait_for_dram_init(ctx, ret_addr, dram_init_check_interval(), max_attempts)
            .await?;

        self.logger.info("DRAM initialized successfully");
        Ok(())
    }

    /// Wait for DRAM initialization to complete
    async fn wait_for_dram_init<C: FelOps>(
        &self,
        ctx: &mut C,
        ret_addr: u32,
        interval: Duration,
        max_attempts: usize,
    ) -> FlashResult<()> {
        self.logger.info("Waiting for DRAM initialization...");
        let start = std::time::Instant::now();
        for attempt in 1..=max_attempts {
            if !interval.is_zero() {
                tokio::time::sleep(interval).await;
            }

            let mut dram_result = vec![0u8; std::mem::size_of::<DramParamInfo>()];
            match ctx.fel_read(ret_addr, &mut dram_result) {
                Ok(_) => {
                    let dram_info = DramParamInfo::parse(&dram_result)
                        .map_err(|e| FlashError::InvalidFirmwareFormat(e.to_string()))?;

                    let dram_init_flag = dram_info.dram_init_flag;
                    let dram_update_flag = dram_info.dram_update_flag;

                    self.logger.debug(&format!(
                        "DRAM init check #{}: init_flag={}, update_flag={}",
                        attempt, dram_init_flag, dram_update_flag
                    ));

                    match dram_init_flag {
                        0 => {}
                        1 => return Err(FlashError::DramInitFailed),
                        _ => {
                            self.logger.debug(&format!(
                                "DRAM init completed after {} attempts, {:?}",
                                attempt,
                                start.elapsed()
                            ));
                            return Ok(());
                        }
                    }
                }
                Err(e) => {
                    self.logger
                        .debug(&format!("DRAM init check #{} failed: {}", attempt, e));
                }
            }
        }

        Err(FlashError::Timeout(format!(
            "DRAM initialization not completed after {} attempts",
            max_attempts
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flash::protocol::tests::MockProtocol;

    fn fes_image(run_addr: u32, ret_addr: u32) -> Vec<u8> {
        let mut data = vec![0u8; std::mem::size_of::<Boot0Header>()];
        let header = Boot0Header::parse_mut(&mut data).unwrap();
        header.magic = *b"eGON.BT0";
        header.run_addr = run_addr;
        header.ret_addr = ret_addr;
        data
    }

    fn dram_response(flag: u32) -> Vec<u8> {
        let mut info = DramParamInfo::create_empty();
        info.dram_init_flag = flag;
        info.serialize()
    }

    #[tokio::test]
    async fn execute_writes_parameters_fes_and_executes_then_waits() {
        let logger = Logger::for_events(false, crate::flash::FlashEventSink::none());
        let handler = DramInit::new(&logger);
        let mut ctx = MockProtocol::default();
        ctx.fel_reads.borrow_mut().push_back(Ok(dram_response(2)));

        handler
            .execute(&mut ctx, &fes_image(0x1000, 0x2000))
            .await
            .unwrap();

        let writes = ctx.fel_writes.borrow();
        assert_eq!(writes.len(), 2);
        assert_eq!(writes[0].addr, 0x2000);
        assert_eq!(writes[1].addr, 0x1000);
        assert_eq!(&*ctx.fel_execs.borrow(), &[0x1000]);
    }

    #[tokio::test]
    async fn execute_rejects_short_fes_and_propagates_transport_errors() {
        let logger = Logger::for_events(false, crate::flash::FlashEventSink::none());
        let handler = DramInit::new(&logger);
        let mut ctx = MockProtocol::default();
        assert!(matches!(
            handler.execute(&mut ctx, &[]).await,
            Err(FlashError::InvalidFirmwareFormat(_))
        ));

        *ctx.fail_fel_write.borrow_mut() = Some("write failed".to_string());
        assert!(matches!(
            handler.execute(&mut ctx, &fes_image(1, 2)).await,
            Err(FlashError::UsbTransferError(_))
        ));

        let mut ctx = MockProtocol::default();
        *ctx.fail_fel_exec.borrow_mut() = Some("exec failed".to_string());
        assert!(matches!(
            handler.execute(&mut ctx, &fes_image(1, 2)).await,
            Err(FlashError::UsbTransferError(_))
        ));
    }

    #[tokio::test]
    async fn wait_handles_read_errors_failure_success_and_timeout() {
        let logger = Logger::for_events(false, crate::flash::FlashEventSink::none());
        let handler = DramInit::new(&logger);

        let mut ctx = MockProtocol::default();
        ctx.fel_reads
            .borrow_mut()
            .extend([Err("temporary".to_string()), Ok(dram_response(2))]);
        handler
            .wait_for_dram_init(&mut ctx, 0, Duration::ZERO, 2)
            .await
            .unwrap();

        let mut failed = MockProtocol::default();
        failed
            .fel_reads
            .borrow_mut()
            .push_back(Ok(dram_response(1)));
        assert!(matches!(
            handler
                .wait_for_dram_init(&mut failed, 0, Duration::ZERO, 1)
                .await,
            Err(FlashError::DramInitFailed)
        ));

        let mut timeout = MockProtocol::default();
        assert!(matches!(
            handler
                .wait_for_dram_init(&mut timeout, 0, Duration::ZERO, 2)
                .await,
            Err(FlashError::Timeout(_))
        ));
    }
}
