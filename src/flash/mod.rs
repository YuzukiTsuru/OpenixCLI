#![allow(dead_code)]

pub mod fel_handler;
pub mod fes_handler;
pub mod progress;
pub mod types;

pub use fel_handler::FelHandler;
pub use fes_handler::FesHandler;

use crate::firmware::OpenixPacker;
use crate::utils::{FlashError, FlashResult, Logger};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FlashMode {
    Partition,
    KeepData,
    PartitionErase,
    FullErase,
}

impl FlashMode {
    pub fn erase_flag(&self) -> u32 {
        match self {
            FlashMode::Partition => 0x0,
            FlashMode::KeepData => 0x0,
            FlashMode::PartitionErase => 0x1,
            FlashMode::FullErase => 0x12,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FlashOptions {
    pub bus: Option<u8>,
    pub port: Option<u8>,
    pub verify: bool,
    pub mode: FlashMode,
    pub partitions: Option<Vec<String>>,
    pub post_action: String,
}

pub struct Flasher {
    packer: OpenixPacker,
    options: FlashOptions,
    logger: Logger,
}

impl Flasher {
    pub fn new(packer: OpenixPacker, options: FlashOptions, logger: Logger) -> Self {
        Self {
            packer,
            options,
            logger,
        }
    }

    pub async fn execute(&mut self) -> FlashResult<()> {
        let total_stages = 6;

        self.logger.stage(1, total_stages, "Preparing FES...");
        let fes_data = self.packer.get_fes().map_err(|_| FlashError::FesNotFound)?;
        self.logger
            .stage_complete(&format!("FES data loaded ({} bytes)", fes_data.len()));

        let mut ctx = if let (Some(bus), Some(port)) = (self.options.bus, self.options.port) {
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

        let mode = ctx.get_device_mode();
        self.logger.info(&format!("Device mode: {:?}", mode));

        if mode != libefex::DeviceMode::Fel {
            self.logger
                .info("Device is not in FEL mode, skipping FEL handler");
        } else {
            self.logger.stage(2, total_stages, "Initializing DRAM...");
            let fel_handler = FelHandler::new(&self.logger);
            fel_handler.handle(&mut ctx, &fes_data).await?;
            self.logger.stage_complete("DRAM initialized successfully");

            self.logger.stage(3, total_stages, "Downloading U-Boot...");

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
                .stage_complete(&format!("U-Boot downloaded ({} bytes)", uboot_data.len()));

            self.logger.stage(4, total_stages, "Reconnecting...");

            ctx = self.reconnect_device().await?;

            self.logger.stage_complete("Device reconnected in FES mode");
        }

        self.logger.stage(5, total_stages, "Flashing partitions...");

        let mut fes_handler = FesHandler::new(&mut self.logger);

        fes_handler
            .handle(&ctx, &mut self.packer, &self.options)
            .await?;

        self.logger.finish_progress();

        self.logger.stage_complete("All partitions flashed");

        self.logger.stage(6, total_stages, "Setting device mode...");

        self.set_device_mode(&ctx).await?;

        self.logger
            .stage_complete(&format!("Device will {}", self.options.post_action));

        Ok(())
    }

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

    async fn set_device_mode(&self, ctx: &libefex::Context) -> FlashResult<()> {
        let tool_mode = match self.options.post_action.as_str() {
            "reboot" => libefex::FesToolMode::Reboot,
            "poweroff" => libefex::FesToolMode::PowerOff,
            "shutdown" => libefex::FesToolMode::PowerOff,
            _ => libefex::FesToolMode::Reboot,
        };

        ctx.fes_tool_mode(libefex::FesToolMode::Normal, tool_mode)
            .map_err(|e| FlashError::UsbTransferError(e.to_string()))?;

        Ok(())
    }
}
