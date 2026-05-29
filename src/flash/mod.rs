//! Flash module
//!
//! Provides flash functionality for writing firmware to Allwinner devices
//! Supports both FEL mode (USB boot) and FES mode (U-Boot)

#![allow(dead_code)]

pub mod events;
pub mod fel_handler;
pub mod fes_handler;
pub mod request;

pub use events::{FlashEvent, FlashEventSink, FlashLogLevel};
pub use fel_handler::FelHandler;
pub use fes_handler::FesHandler;
pub use request::{DeviceSelector, FlashMode, FlashRequest, PostAction};

use crate::firmware::OpenixPacker;
use crate::process::{FlashStages, StageType};
use crate::utils::{FlashError, FlashResult, Logger};

/// Main flash controller
///
/// Coordinates the flashing process including FEL initialization,
/// FES handling, and partition flashing
pub struct Flasher {
    packer: OpenixPacker,
    request: FlashRequest,
    logger: Logger,
}

impl Flasher {
    /// Create a new flasher instance
    pub fn new(packer: OpenixPacker, request: FlashRequest, logger: Logger) -> Self {
        Self {
            packer,
            request,
            logger,
        }
    }

    /// Execute the flash process
    ///
    /// This is the main entry point for the flashing process.
    /// It handles both FEL and FES mode devices.
    pub async fn execute(&mut self) -> FlashResult<()> {
        let fes_data = self.packer.get_fes().map_err(|_| FlashError::FesNotFound)?;

        let mut ctx = self.open_device()?;

        let mode = ctx.get_device_mode();
        self.logger.info(&format!("Device mode: {:?}", mode));

        let has_fel = mode == libefex::DeviceMode::Fel;

        let stages = if has_fel {
            FlashStages::for_fel_mode()
        } else {
            FlashStages::for_fes_mode()
        };
        self.logger.define_stages(stages.stages());

        self.logger.start_global_progress();

        self.logger.begin_stage(StageType::Init);
        self.logger
            .info(&format!("FES data loaded ({} bytes)", fes_data.len()));
        self.logger.complete_stage();

        if has_fel {
            ctx = self.prepare_fel_mode(ctx, &fes_data).await?;
        }

        self.run_fes_mode(&ctx).await?;
        self.apply_post_action(&ctx).await?;

        Ok(())
    }

    /// Open the selected device, or the first detected device when no full selector is provided.
    fn open_device(&self) -> FlashResult<libefex::Context> {
        let mut ctx = if let Some((bus, port)) = self.request.device.selected_pair() {
            let mut ctx = libefex::Context::new();
            ctx.scan_usb_device_at(bus, port)
                .map_err(|e| FlashError::DeviceOpenFailed(e.to_string()))?;
            ctx
        } else {
            let devices = libefex::Context::scan_usb_devices()
                .map_err(|e| FlashError::DeviceOpenFailed(e.to_string()))?;

            if devices.is_empty() {
                return Err(FlashError::DeviceNotFound);
            }

            let mut ctx = libefex::Context::new();
            ctx.scan_usb_device_at(devices[0].bus, devices[0].port)
                .map_err(|e| FlashError::DeviceOpenFailed(e.to_string()))?;
            ctx
        };

        ctx.usb_init()
            .map_err(|e| FlashError::DeviceOpenFailed(e.to_string()))?;

        ctx.efex_init()
            .map_err(|e| FlashError::DeviceOpenFailed(e.to_string()))?;

        Ok(ctx)
    }

    async fn prepare_fel_mode(
        &mut self,
        mut ctx: libefex::Context,
        fes_data: &[u8],
    ) -> FlashResult<libefex::Context> {
        self.logger.begin_stage(StageType::FelDram);
        let fel_handler = FelHandler::new(&self.logger);
        fel_handler.handle(&mut ctx, fes_data).await?;
        self.logger.complete_stage();

        self.logger.begin_stage(StageType::FelUboot);

        let uboot_data = self
            .packer
            .get_uboot()
            .map_err(|_| FlashError::UbootNotFound)?;

        let dtb_data = self.packer.get_dtb().ok();

        let sysconfig_data = self
            .packer
            .get_sys_config_bin()
            .map_err(|_| FlashError::SysConfigNotFound)?;

        let board_config_data = self.packer.get_board_config().ok();

        fel_handler
            .download_uboot(
                &ctx,
                &uboot_data,
                dtb_data.as_deref(),
                &sysconfig_data,
                board_config_data.as_deref(),
            )
            .await?;

        self.logger
            .info(&format!("U-Boot downloaded ({} bytes)", uboot_data.len()));
        self.logger.complete_stage();

        self.logger.begin_stage(StageType::FelReconnect);
        let ctx = self.reconnect_device().await?;
        self.logger.complete_stage();

        Ok(ctx)
    }

    async fn run_fes_mode(&mut self, ctx: &libefex::Context) -> FlashResult<()> {
        let mut fes_handler = FesHandler::new(&mut self.logger);
        fes_handler
            .handle(ctx, &mut self.packer, &self.request)
            .await
    }

    async fn apply_post_action(&self, ctx: &libefex::Context) -> FlashResult<()> {
        self.logger.begin_stage(StageType::FesMode);
        self.set_device_mode(ctx).await?;
        self.logger.complete_stage();

        self.logger
            .stage_complete(&format!("Device will {}", self.request.post_action));
        self.logger.flash_finished(self.request.post_action);
        self.logger.finish_progress();

        Ok(())
    }

    /// Reconnect to device after FEL mode operations
    async fn reconnect_device(&self) -> FlashResult<libefex::Context> {
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        let max_retries = 25;
        let mut retries = 0;

        while retries < max_retries {
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

            let devices = match libefex::Context::scan_usb_devices() {
                Ok(d) => d,
                Err(_) => {
                    retries += 1;
                    self.logger.debug(&format!(
                        "Reconnect attempt {}/{} (scan failed)",
                        retries, max_retries
                    ));
                    continue;
                }
            };

            for dev in devices {
                let mut new_ctx = libefex::Context::new();
                if new_ctx.scan_usb_device_at(dev.bus, dev.port).is_err() {
                    continue;
                }
                if new_ctx.usb_init().is_err() {
                    continue;
                }
                if new_ctx.efex_init().is_err() {
                    continue;
                }

                if new_ctx.get_device_mode() == libefex::DeviceMode::Srv {
                    self.logger.debug(&format!(
                        "Device found at bus {}, port {}",
                        dev.bus, dev.port
                    ));
                    return Ok(new_ctx);
                }
            }

            retries += 1;
            self.logger
                .debug(&format!("Reconnect attempt {}/{}", retries, max_retries));
        }

        Err(FlashError::ReconnectFailed)
    }

    /// Set device mode after flashing
    async fn set_device_mode(&self, ctx: &libefex::Context) -> FlashResult<()> {
        let tool_mode = self.request.post_action.fes_tool_mode();

        ctx.fes_tool_mode(libefex::FesToolMode::Normal, tool_mode)
            .map_err(|e| FlashError::UsbTransferError(e.to_string()))?;

        Ok(())
    }
}
