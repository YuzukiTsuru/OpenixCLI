//! Scan command implementation
//!
//! Scans for connected Allwinner devices via USB

use colored::Colorize;
use libefex::{Context, DeviceMode};

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
    println!("{}", "Scanning USB devices...".cyan().bold());
    println!();

    let devices = Context::scan_usb_devices()?;

    if devices.is_empty() {
        println!("{}", "No devices found.".yellow());
        return Ok(());
    }

    println!("Found {} device(s):\n", devices.len());

    for (idx, dev) in devices.iter().enumerate() {
        println!(
            "[{}] {} {}, Port {:03}",
            (idx + 1).to_string().cyan(),
            format!("Bus {:03}", dev.bus).white(),
            format!(", Port {:03}", dev.port).white(),
            dev.port
        );

        if detailed {
            let mut ctx = Context::new();
            if ctx.scan_usb_device_at(dev.bus, dev.port).is_err() {
                println!("    {}", "Failed to initialize device".red());
                println!();
                continue;
            }
            if ctx.usb_init().is_err() {
                println!("    {}", "Failed to initialize USB".red());
                println!();
                continue;
            }
            if ctx.efex_init().is_err() {
                println!("    {}", "Failed to initialize EFEX".red());
                println!();
                continue;
            }

            let mode_str = ctx.get_device_mode_str().to_string();
            let chip_version = unsafe { (*ctx.as_ptr()).resp.id };

            println!(
                "    Chip: {} (0x{:08x})",
                mode_str.white().bold(),
                chip_version
            );
            println!(
                "    Mode: {}",
                match ctx.get_device_mode() {
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
