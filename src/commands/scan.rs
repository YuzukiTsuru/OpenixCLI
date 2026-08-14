//! Scan command implementation
//!
//! Scans for connected Allwinner devices via USB

use colored::Colorize;
use libefex::{Context, DeviceMode};

#[derive(Debug, Clone, Copy)]
struct ScanDevice {
    bus: u8,
    port: u8,
}

struct DeviceDetails {
    mode_str: String,
    chip_version: u32,
    mode: DeviceMode,
}

trait ScanBackend {
    fn scan(&self) -> anyhow::Result<Vec<ScanDevice>>;
    fn inspect(&self, device: ScanDevice) -> anyhow::Result<DeviceDetails>;
}

struct LibefexScanBackend;

impl ScanBackend for LibefexScanBackend {
    fn scan(&self) -> anyhow::Result<Vec<ScanDevice>> {
        Ok(Context::scan_usb_devices()?
            .into_iter()
            .map(|device| ScanDevice {
                bus: device.bus,
                port: device.port,
            })
            .collect())
    }

    fn inspect(&self, device: ScanDevice) -> anyhow::Result<DeviceDetails> {
        let mut context = Context::new();
        context.scan_usb_device_at(device.bus, device.port)?;
        context.usb_init()?;
        context.efex_init()?;

        Ok(DeviceDetails {
            mode_str: context.get_device_mode_str().to_string(),
            chip_version: unsafe { (*context.as_ptr()).resp.id },
            mode: context.get_device_mode(),
        })
    }
}

/// Execute the scan command
///
/// Scans for USB devices and displays information about connected Allwinner devices
///
/// # Arguments
/// * `detailed` - If true, initialize device context to get detailed information
///
/// # Returns
/// Ok(()) on success, Error on failure
pub async fn execute(detailed: bool) -> anyhow::Result<()> {
    execute_with(detailed, &LibefexScanBackend).await
}

async fn execute_with<B: ScanBackend>(detailed: bool, backend: &B) -> anyhow::Result<()> {
    println!("{}", "Scanning USB devices...".cyan().bold());
    println!();

    let devices = backend.scan()?;

    if devices.is_empty() {
        println!("{}", "No devices found.".yellow());
        return Ok(());
    }

    println!("Found {} device(s):\n", devices.len());

    for (idx, dev) in devices.iter().enumerate() {
        println!(
            "[{}] {}, Port {:03}",
            (idx + 1).to_string().cyan(),
            format!("Bus {:03}", dev.bus).white(),
            dev.port
        );

        if detailed {
            let details = match backend.inspect(*dev) {
                Ok(details) => details,
                Err(_) => {
                    println!("    {}", "Failed to initialize device".red());
                    println!();
                    continue;
                }
            };

            println!(
                "    Chip: {} (0x{:08x})",
                details.mode_str.white().bold(),
                details.chip_version
            );
            println!(
                "    Mode: {}",
                match details.mode {
                    DeviceMode::Fel => "FEL (USB Boot)",
                    DeviceMode::Srv => "FES (U-Boot)",
                    _ => "Unknown",
                }
            );
        }
        println!();
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    struct MockScanBackend {
        scan_result: RefCell<anyhow::Result<Vec<ScanDevice>>>,
        details: RefCell<Vec<anyhow::Result<DeviceDetails>>>,
        inspect_calls: Cell<usize>,
    }

    impl MockScanBackend {
        fn with_devices(devices: Vec<ScanDevice>) -> Self {
            Self {
                scan_result: RefCell::new(Ok(devices)),
                details: RefCell::new(Vec::new()),
                inspect_calls: Cell::new(0),
            }
        }
    }

    impl ScanBackend for MockScanBackend {
        fn scan(&self) -> anyhow::Result<Vec<ScanDevice>> {
            self.scan_result.replace(Ok(Vec::new()))
        }

        fn inspect(&self, _device: ScanDevice) -> anyhow::Result<DeviceDetails> {
            self.inspect_calls.set(self.inspect_calls.get() + 1);
            if self.details.borrow().is_empty() {
                return Err(anyhow::anyhow!("no details"));
            }
            self.details.borrow_mut().remove(0)
        }
    }

    #[tokio::test]
    async fn scan_core_handles_empty_summary_and_scan_errors() {
        let empty = MockScanBackend::with_devices(Vec::new());
        execute_with(false, &empty).await.unwrap();
        assert_eq!(empty.inspect_calls.get(), 0);

        let summary = MockScanBackend::with_devices(vec![ScanDevice { bus: 1, port: 2 }]);
        execute_with(false, &summary).await.unwrap();
        assert_eq!(summary.inspect_calls.get(), 0);

        let failed = MockScanBackend {
            scan_result: RefCell::new(Err(anyhow::anyhow!("scan failed"))),
            details: RefCell::new(Vec::new()),
            inspect_calls: Cell::new(0),
        };
        assert_eq!(
            execute_with(false, &failed).await.unwrap_err().to_string(),
            "scan failed"
        );
    }

    #[tokio::test]
    async fn detailed_scan_handles_failed_fel_fes_and_unknown_devices() {
        let backend = MockScanBackend::with_devices(vec![
            ScanDevice { bus: 1, port: 1 },
            ScanDevice { bus: 1, port: 2 },
            ScanDevice { bus: 1, port: 3 },
            ScanDevice { bus: 1, port: 4 },
        ]);
        backend.details.borrow_mut().extend([
            Err(anyhow::anyhow!("open failed")),
            Ok(DeviceDetails {
                mode_str: "chip-fel".into(),
                chip_version: 1,
                mode: DeviceMode::Fel,
            }),
            Ok(DeviceDetails {
                mode_str: "chip-fes".into(),
                chip_version: 2,
                mode: DeviceMode::Srv,
            }),
            Ok(DeviceDetails {
                mode_str: "chip-other".into(),
                chip_version: 3,
                mode: DeviceMode::Null,
            }),
        ]);
        execute_with(true, &backend).await.unwrap();
        assert_eq!(backend.inspect_calls.get(), 4);
    }
}
