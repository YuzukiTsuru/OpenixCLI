//! Flash module
//!
//! Provides flash functionality for writing firmware to Allwinner devices
//! Supports both FEL mode (USB boot) and FES mode (U-Boot)

#![allow(dead_code)]

pub mod events;
pub mod fel_handler;
pub mod fes_handler;
pub mod protocol;
pub mod request;

pub use events::{FlashEvent, FlashEventSink, FlashLogLevel};
pub use fel_handler::FelHandler;
pub use fes_handler::FesHandler;
pub use request::{
    CustomFlashLayout, DeviceSelector, ExternalPartition, FlashMode, FlashRequest, PostAction,
};

use crate::firmware::OpenixPacker;
use crate::flash::protocol::{FelOps, FesOps};
use crate::process::{FlashStages, StageType};
use crate::utils::{FlashError, FlashResult, Logger};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[doc(hidden)]
pub struct DeviceLocation {
    pub bus: u8,
    pub port: u8,
}

#[doc(hidden)]
pub trait DeviceBackend {
    type Context: FelOps + FesOps;

    fn scan_devices(&self) -> Result<Vec<DeviceLocation>, String>;
    fn open_device(&self, location: DeviceLocation) -> Result<Self::Context, String>;
    fn device_mode(&self, context: &Self::Context) -> libefex::DeviceMode;
}

#[derive(Default)]
#[doc(hidden)]
pub struct LibefexDeviceBackend;

impl DeviceBackend for LibefexDeviceBackend {
    type Context = libefex::Context;

    fn scan_devices(&self) -> Result<Vec<DeviceLocation>, String> {
        libefex::Context::scan_usb_devices()
            .map(|devices| {
                devices
                    .into_iter()
                    .map(|device| DeviceLocation {
                        bus: device.bus,
                        port: device.port,
                    })
                    .collect()
            })
            .map_err(|error| error.to_string())
    }

    fn open_device(&self, location: DeviceLocation) -> Result<Self::Context, String> {
        let mut context = libefex::Context::new();
        context
            .scan_usb_device_at(location.bus, location.port)
            .map_err(|error| error.to_string())?;
        context.usb_init().map_err(|error| error.to_string())?;
        context.efex_init().map_err(|error| error.to_string())?;
        Ok(context)
    }

    fn device_mode(&self, context: &Self::Context) -> libefex::DeviceMode {
        context.get_device_mode()
    }
}

#[derive(Debug, Clone, Copy)]
struct ReconnectPolicy {
    initial_delay: tokio::time::Duration,
    retry_delay: tokio::time::Duration,
    startup_timeout: tokio::time::Duration,
}

impl Default for ReconnectPolicy {
    fn default() -> Self {
        Self {
            initial_delay: tokio::time::Duration::from_secs(2),
            retry_delay: tokio::time::Duration::from_secs(1),
            startup_timeout: tokio::time::Duration::from_secs(45),
        }
    }
}

/// Main flash controller
///
/// Coordinates the flashing process including FEL initialization,
/// FES handling, and partition flashing
pub struct Flasher<B = LibefexDeviceBackend> {
    packer: OpenixPacker,
    request: FlashRequest,
    logger: Logger,
    backend: B,
    reconnect_policy: ReconnectPolicy,
}

impl Flasher<LibefexDeviceBackend> {
    /// Create a new flasher instance
    pub fn new(packer: OpenixPacker, request: FlashRequest, logger: Logger) -> Self {
        Self {
            packer,
            request,
            logger,
            backend: LibefexDeviceBackend,
            reconnect_policy: ReconnectPolicy::default(),
        }
    }
}

impl<B: DeviceBackend> Flasher<B> {
    #[cfg(test)]
    fn with_backend(
        packer: OpenixPacker,
        request: FlashRequest,
        logger: Logger,
        backend: B,
    ) -> Self {
        Self {
            packer,
            request,
            logger,
            backend,
            reconnect_policy: ReconnectPolicy {
                initial_delay: tokio::time::Duration::ZERO,
                retry_delay: tokio::time::Duration::from_millis(1),
                startup_timeout: tokio::time::Duration::from_millis(100),
            },
        }
    }

    /// Execute the flash process
    ///
    /// This is the main entry point for the flashing process.
    /// It handles both FEL and FES mode devices.
    pub async fn execute(&mut self) -> FlashResult<()> {
        let result = self.execute_inner().await;
        if result.is_err() {
            self.logger.finish_progress();
        }
        result
    }

    async fn execute_inner(&mut self) -> FlashResult<()> {
        let fes_data = self.packer.get_fes().map_err(|_| FlashError::FesNotFound)?;

        let mut ctx = self.open_device()?;

        let mode = self.backend.device_mode(&ctx);
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
    fn open_device(&self) -> FlashResult<B::Context> {
        let location = if let Some((bus, port)) = self.request.device.selected_pair() {
            DeviceLocation { bus, port }
        } else {
            let devices = self
                .backend
                .scan_devices()
                .map_err(|e| FlashError::DeviceOpenFailed(e.to_string()))?;

            devices.first().copied().ok_or(FlashError::DeviceNotFound)?
        };

        self.backend
            .open_device(location)
            .map_err(FlashError::DeviceOpenFailed)
    }

    async fn prepare_fel_mode(
        &mut self,
        mut ctx: B::Context,
        fes_data: &[u8],
    ) -> FlashResult<B::Context> {
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
            .info(&format!("U-Boot transferred ({} bytes)", uboot_data.len()));
        self.logger.complete_stage();

        self.logger.begin_stage(StageType::FelReconnect);
        let ctx = self.reconnect_device().await?;
        self.logger.complete_stage();

        Ok(ctx)
    }

    async fn run_fes_mode(&mut self, ctx: &B::Context) -> FlashResult<()> {
        let mut fes_handler = FesHandler::new(&mut self.logger);
        fes_handler
            .handle(ctx, &mut self.packer, &self.request)
            .await
    }

    async fn apply_post_action(&self, ctx: &B::Context) -> FlashResult<()> {
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
    async fn reconnect_device(&self) -> FlashResult<B::Context> {
        let timeout = self.reconnect_policy.startup_timeout;
        let deadline = tokio::time::Instant::now() + timeout;
        let timeout_seconds = timeout.as_secs();

        self.logger.info(&format!(
            "Waiting up to {timeout_seconds}s for U-Boot to initialize the FES USB device..."
        ));

        if tokio::time::timeout_at(
            deadline,
            tokio::time::sleep(self.reconnect_policy.initial_delay),
        )
        .await
        .is_err()
        {
            return Err(FlashError::UbootStartupTimeout {
                seconds: timeout_seconds,
            });
        }

        let mut attempts = 0;

        loop {
            attempts += 1;

            let devices = match self.backend.scan_devices() {
                Ok(d) => d,
                Err(_) => {
                    self.logger
                        .debug(&format!("Reconnect attempt {attempts} (scan failed)"));
                    Vec::new()
                }
            };

            let found_devices = !devices.is_empty();
            for dev in devices {
                let Ok(new_ctx) = self.backend.open_device(dev) else {
                    continue;
                };

                if self.backend.device_mode(&new_ctx) == libefex::DeviceMode::Srv {
                    self.logger.debug(&format!(
                        "Device found at bus {}, port {}",
                        dev.bus, dev.port
                    ));
                    return Ok(new_ctx);
                }
            }

            if found_devices {
                self.logger.debug(&format!(
                    "Reconnect attempt {attempts} (FES device not ready)"
                ));
            }

            if tokio::time::Instant::now() >= deadline {
                break;
            }

            if tokio::time::timeout_at(
                deadline,
                tokio::time::sleep(self.reconnect_policy.retry_delay),
            )
            .await
            .is_err()
            {
                break;
            }
        }

        Err(FlashError::UbootStartupTimeout {
            seconds: timeout_seconds,
        })
    }

    /// Set device mode after flashing
    async fn set_device_mode(&self, ctx: &B::Context) -> FlashResult<()> {
        let tool_mode = self.request.post_action.fes_tool_mode();

        ctx.fes_tool_mode(libefex::FesToolMode::Normal, tool_mode)
            .map_err(|e| FlashError::UsbTransferError(e.to_string()))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::boot_header::{Boot0Header, UBootHeader};
    use crate::config::sys_config::DramParamInfo;
    use crate::flash::protocol::tests::MockProtocol;
    use crate::test_support::{mbr_bytes, test_firmware, FirmwareEntry};
    use libefex::{DeviceMode, FesDataType, FesToolMode};
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::rc::Rc;
    use std::sync::{Arc, Mutex};

    struct MockDeviceBackend {
        context: Rc<MockProtocol>,
        scans: RefCell<VecDeque<Result<Vec<DeviceLocation>, String>>>,
        opens: RefCell<VecDeque<Result<(), String>>>,
        modes: RefCell<VecDeque<DeviceMode>>,
        fallback_mode: Cell<DeviceMode>,
        open_calls: RefCell<Vec<DeviceLocation>>,
    }

    impl Default for MockDeviceBackend {
        fn default() -> Self {
            Self {
                context: Rc::new(MockProtocol::default()),
                scans: RefCell::new(VecDeque::new()),
                opens: RefCell::new(VecDeque::new()),
                modes: RefCell::new(VecDeque::new()),
                fallback_mode: Cell::new(DeviceMode::Srv),
                open_calls: RefCell::new(Vec::new()),
            }
        }
    }

    impl DeviceBackend for MockDeviceBackend {
        type Context = Rc<MockProtocol>;

        fn scan_devices(&self) -> Result<Vec<DeviceLocation>, String> {
            self.scans
                .borrow_mut()
                .pop_front()
                .unwrap_or_else(|| Ok(vec![DeviceLocation { bus: 1, port: 2 }]))
        }

        fn open_device(&self, location: DeviceLocation) -> Result<Self::Context, String> {
            self.open_calls.borrow_mut().push(location);
            self.opens.borrow_mut().pop_front().unwrap_or(Ok(()))?;
            Ok(Rc::clone(&self.context))
        }

        fn device_mode(&self, _context: &Self::Context) -> DeviceMode {
            self.modes
                .borrow_mut()
                .pop_front()
                .unwrap_or_else(|| self.fallback_mode.get())
        }
    }

    fn request(selector: DeviceSelector, post_action: PostAction) -> FlashRequest {
        FlashRequest::new(selector, false, FlashMode::FullErase, None, post_action)
    }

    fn loaded_packer(entries: &[FirmwareEntry<'_>]) -> OpenixPacker {
        let firmware = test_firmware(entries);
        let mut packer = OpenixPacker::new();
        packer.load(firmware.path()).unwrap();
        packer
    }

    fn fes_firmware(extra_entries: &[FirmwareEntry<'_>]) -> OpenixPacker {
        let mbr = mbr_bytes(&[("system", 0x20, 3, false)]);
        let mut entries = vec![
            FirmwareEntry {
                filename: "fes.fex",
                maintype: "FES",
                subtype: "FES_1-0000000000",
                data: b"fes",
            },
            FirmwareEntry {
                filename: "sunxi_mbr.fex",
                maintype: "12345678",
                subtype: "1234567890___MBR",
                data: &mbr,
            },
            FirmwareEntry {
                filename: "sys_partition.fex",
                maintype: "COMMON",
                subtype: "SYS_CONFIG000000",
                data: b"[partition_start]\n[partition]\nname=system\ndownloadfile=system.img\n",
            },
            FirmwareEntry {
                filename: "system.img",
                maintype: "RFSFAT16",
                subtype: "SYSTEM_IMG000000",
                data: b"raw",
            },
            FirmwareEntry {
                filename: "preboot.fex",
                maintype: "12345678",
                subtype: "1234567890PREB_0",
                data: b"preboot",
            },
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
        ];
        entries.extend_from_slice(extra_entries);
        loaded_packer(&entries)
    }

    fn event_logger(events: Arc<Mutex<Vec<FlashEvent>>>) -> Logger {
        Logger::for_events(
            true,
            FlashEventSink::from_fn(move |event| events.lock().unwrap().push(event)),
        )
    }

    #[test]
    fn open_device_covers_explicit_fallback_empty_scan_and_transport_errors() {
        let logger = Logger::for_events(false, FlashEventSink::none());

        let explicit_backend = MockDeviceBackend::default();
        let explicit = Flasher::with_backend(
            OpenixPacker::new(),
            request(DeviceSelector::new(Some(7), Some(8)), PostAction::Reboot),
            logger.clone(),
            explicit_backend,
        );
        explicit.open_device().unwrap();
        assert_eq!(
            &*explicit.backend.open_calls.borrow(),
            &[DeviceLocation { bus: 7, port: 8 }]
        );
        assert!(explicit.backend.scans.borrow().is_empty());

        let fallback_backend = MockDeviceBackend::default();
        fallback_backend
            .scans
            .borrow_mut()
            .push_back(Ok(vec![DeviceLocation { bus: 3, port: 4 }]));
        let fallback = Flasher::with_backend(
            OpenixPacker::new(),
            request(DeviceSelector::new(Some(7), None), PostAction::Reboot),
            logger.clone(),
            fallback_backend,
        );
        fallback.open_device().unwrap();
        assert_eq!(
            &*fallback.backend.open_calls.borrow(),
            &[DeviceLocation { bus: 3, port: 4 }]
        );

        for (scan, expected) in [
            (Ok(Vec::new()), "Device not found"),
            (
                Err("scan failed".into()),
                "Failed to open device: scan failed",
            ),
        ] {
            let backend = MockDeviceBackend::default();
            backend.scans.borrow_mut().push_back(scan);
            let flasher = Flasher::with_backend(
                OpenixPacker::new(),
                request(DeviceSelector::default(), PostAction::Reboot),
                logger.clone(),
                backend,
            );
            assert_eq!(flasher.open_device().err().unwrap().to_string(), expected);
        }

        let backend = MockDeviceBackend::default();
        backend
            .opens
            .borrow_mut()
            .push_back(Err("init failed".into()));
        let flasher = Flasher::with_backend(
            OpenixPacker::new(),
            request(DeviceSelector::new(Some(1), Some(2)), PostAction::Reboot),
            logger,
            backend,
        );
        assert_eq!(
            flasher.open_device().err().unwrap().to_string(),
            "Failed to open device: init failed"
        );
    }

    #[tokio::test]
    async fn reconnect_retries_scan_open_and_wrong_mode_then_reports_startup_timeout() {
        let logger = Logger::for_events(false, FlashEventSink::none());
        let backend = MockDeviceBackend::default();
        backend.scans.borrow_mut().extend([
            Err("scan".into()),
            Ok(vec![DeviceLocation { bus: 1, port: 2 }]),
            Ok(vec![DeviceLocation { bus: 3, port: 4 }]),
        ]);
        backend
            .opens
            .borrow_mut()
            .extend([Err("open".into()), Ok(())]);
        backend.modes.borrow_mut().push_back(DeviceMode::Srv);
        let flasher = Flasher::with_backend(
            OpenixPacker::new(),
            request(DeviceSelector::default(), PostAction::Reboot),
            logger.clone(),
            backend,
        );
        flasher.reconnect_device().await.unwrap();
        assert_eq!(flasher.backend.open_calls.borrow().len(), 2);

        let backend = MockDeviceBackend::default();
        backend
            .modes
            .borrow_mut()
            .extend([DeviceMode::Fel, DeviceMode::Fel, DeviceMode::Fel]);
        backend.fallback_mode.set(DeviceMode::Fel);
        let flasher = Flasher::with_backend(
            OpenixPacker::new(),
            request(DeviceSelector::default(), PostAction::Reboot),
            logger,
            backend,
        );
        assert!(matches!(
            flasher.reconnect_device().await,
            Err(FlashError::UbootStartupTimeout { .. })
        ));
    }

    #[tokio::test]
    async fn post_action_maps_protocol_mode_and_propagates_transfer_errors() {
        let logger = Logger::for_events(false, FlashEventSink::none());
        let backend = MockDeviceBackend::default();
        let context = Rc::clone(&backend.context);
        let flasher = Flasher::with_backend(
            OpenixPacker::new(),
            request(DeviceSelector::default(), PostAction::PowerOff),
            logger.clone(),
            backend,
        );
        flasher.set_device_mode(&context).await.unwrap();
        assert_eq!(
            &*context.tool_modes.borrow(),
            &[(FesToolMode::Normal, FesToolMode::PowerOff)]
        );

        *context.fail_tool_mode.borrow_mut() = Some("mode failed".into());
        assert_eq!(
            flasher
                .set_device_mode(&context)
                .await
                .unwrap_err()
                .to_string(),
            "USB transfer error: mode failed"
        );
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn direct_fes_execute_downloads_preboot_before_boot_images_and_finishes() {
        let _guard = crate::test_support::GLOBAL_STATE_TEST_LOCK.lock().unwrap();
        crate::process::global_progress::set_tui_mode(true);
        let backend = MockDeviceBackend::default();
        backend.modes.borrow_mut().push_back(DeviceMode::Srv);
        let protocol = Rc::clone(&backend.context);
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut flasher = Flasher::with_backend(
            fes_firmware(&[]),
            request(DeviceSelector::default(), PostAction::Reboot),
            event_logger(Arc::clone(&events)),
            backend,
        );
        flasher.execute().await.unwrap();

        let download_types: Vec<_> = protocol
            .downloads
            .borrow()
            .iter()
            .map(|download| download.data_type)
            .collect();
        let boot_sequence: Vec<_> = download_types
            .into_iter()
            .filter(|kind| {
                matches!(
                    kind,
                    FesDataType::Preboot | FesDataType::Boot1 | FesDataType::Boot0
                )
            })
            .collect();
        assert_eq!(
            boot_sequence,
            [FesDataType::Preboot, FesDataType::Boot1, FesDataType::Boot0]
        );
        assert!(events.lock().unwrap().iter().any(|event| matches!(
            event,
            FlashEvent::Finished {
                post_action: PostAction::Reboot
            }
        )));
        assert!(crate::process::global_progress::global_progress()
            .snapshot()
            .stages
            .is_empty());
        crate::process::global_progress::set_tui_mode(false);
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn fel_execute_initializes_downloads_reconnects_and_enters_fes() {
        let _guard = crate::test_support::GLOBAL_STATE_TEST_LOCK.lock().unwrap();
        crate::process::global_progress::set_tui_mode(true);

        let mut fes = vec![0; std::mem::size_of::<Boot0Header>()];
        let fes_header = Boot0Header::parse_mut(&mut fes).unwrap();
        fes_header.run_addr = 0x1000;
        fes_header.ret_addr = 0x2000;
        let mut uboot = vec![0; std::mem::size_of::<UBootHeader>()];
        UBootHeader::parse_mut(&mut uboot)
            .unwrap()
            .uboot_head
            .run_addr = 0x4000;

        let backend = MockDeviceBackend::default();
        backend
            .modes
            .borrow_mut()
            .extend([DeviceMode::Fel, DeviceMode::Srv]);
        let protocol = Rc::clone(&backend.context);
        let mut dram = DramParamInfo::create_empty();
        dram.dram_init_flag = 2;
        protocol
            .fel_reads
            .borrow_mut()
            .push_back(Ok(dram.serialize()));

        let mbr = mbr_bytes(&[("system", 0x20, 3, false)]);
        let entries = [
            FirmwareEntry {
                filename: "fes.fex",
                maintype: "FES",
                subtype: "FES_1-0000000000",
                data: &fes,
            },
            FirmwareEntry {
                filename: "sunxi_mbr.fex",
                maintype: "12345678",
                subtype: "1234567890___MBR",
                data: &mbr,
            },
            FirmwareEntry {
                filename: "sys_partition.fex",
                maintype: "COMMON",
                subtype: "SYS_CONFIG000000",
                data: b"[partition_start]\n[partition]\nname=system\ndownloadfile=system.img\n",
            },
            FirmwareEntry {
                filename: "system.img",
                maintype: "RFSFAT16",
                subtype: "SYSTEM_IMG000000",
                data: b"raw",
            },
            FirmwareEntry {
                filename: "preboot.fex",
                maintype: "12345678",
                subtype: "1234567890PREB_0",
                data: b"preboot",
            },
            FirmwareEntry {
                filename: "uboot.fex",
                maintype: "12345678",
                subtype: "UBOOT_0000000000",
                data: &uboot,
            },
            FirmwareEntry {
                filename: "boot0.fex",
                maintype: "12345678",
                subtype: "1234567890BUFS_0",
                data: b"boot0",
            },
            FirmwareEntry {
                filename: "sys_config.bin",
                maintype: "COMMON",
                subtype: "SYS_CONFIG_BIN00",
                data: b"sysconfig",
            },
        ];
        let mut flasher = Flasher::with_backend(
            loaded_packer(&entries),
            request(DeviceSelector::default(), PostAction::Reboot),
            Logger::for_events(false, FlashEventSink::none()),
            backend,
        );
        flasher.execute().await.unwrap();

        assert_eq!(&*protocol.fel_execs.borrow(), &[0x1000, 0x4000]);
        assert!(protocol
            .downloads
            .borrow()
            .iter()
            .any(|download| download.data_type == FesDataType::Preboot));
        assert_eq!(flasher.backend.open_calls.borrow().len(), 2);
        crate::process::global_progress::set_tui_mode(false);
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn execute_reports_missing_fes_and_cleans_progress_after_device_errors() {
        let _guard = crate::test_support::GLOBAL_STATE_TEST_LOCK.lock().unwrap();
        crate::process::global_progress::set_tui_mode(true);
        let backend = MockDeviceBackend::default();
        let mut missing = Flasher::with_backend(
            OpenixPacker::new(),
            request(DeviceSelector::default(), PostAction::Reboot),
            Logger::for_events(false, FlashEventSink::none()),
            backend,
        );
        assert!(matches!(
            missing.execute().await,
            Err(FlashError::FesNotFound)
        ));

        let backend = MockDeviceBackend::default();
        *backend.context.storage.borrow_mut() = Err("query failed".into());
        let mut failing = Flasher::with_backend(
            fes_firmware(&[]),
            request(DeviceSelector::default(), PostAction::Reboot),
            Logger::for_events(false, FlashEventSink::none()),
            backend,
        );
        assert_eq!(
            failing.execute().await.unwrap_err().to_string(),
            "USB transfer error: query failed"
        );
        assert!(crate::process::global_progress::global_progress()
            .snapshot()
            .stages
            .is_empty());
        crate::process::global_progress::set_tui_mode(false);
    }
}
