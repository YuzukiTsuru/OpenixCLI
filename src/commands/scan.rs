//! Scan command implementation
//!
//! Scans for connected Allwinner devices via USB

use colored::Colorize;
use libefex::{Context, DeviceMode};

/// Execute the scan command
///
/// Scans for USB devices and displays information about connected Allwinner devices
///
/// # Returns
/// Ok(()) on success, Error on failure
pub async fn execute() -> anyhow::Result<()> {
    println!("{}", "Scanning USB devices...".cyan().bold());
    println!();

    let devices = Context::scan_usb_devices()?;

    if devices.is_empty() {
        println!("{}", "No devices found.".yellow());
        return Ok(());
    }

    println!("Found {} device(s):\n", devices.len());

    for (idx, dev) in devices.iter().enumerate() {
        let mut ctx = Context::new();
        ctx.scan_usb_device_at(dev.bus, dev.port)?;
        ctx.usb_init()?;
        ctx.efex_init()?;

        let mode: String = match ctx.get_device_mode() {
            DeviceMode::Fel => "FEL".red().to_string(),
            DeviceMode::Srv => "FES".green().to_string(),
            DeviceMode::UpdateCool => "UPDATE_COOL".yellow().to_string(),
            DeviceMode::UpdateHot => "UPDATE_HOT".yellow().to_string(),
            DeviceMode::Null => "NULL".white().to_string(),
            DeviceMode::Unknown(v) => format!("UNKNOWN(0x{:04x})", v).white().to_string(),
        };

        let mode_str = ctx.get_device_mode_str().to_string();
        let chip_version = unsafe { (*ctx.as_ptr()).resp.id };

        println!(
            "[{}] {} {}, Port {:03} - {}",
            (idx + 1).to_string().cyan(),
            format!("Bus {:03}", dev.bus).white(),
            format!(", Port {:03}", dev.port).white(),
            dev.port,
            mode
        );
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
        println!();
    }

    Ok(())
}
