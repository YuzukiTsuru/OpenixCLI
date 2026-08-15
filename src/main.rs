//! OpenixCLI-cli - Firmware flashing CLI tool for Allwinner chips
//!
//! This tool provides the following functionality:
//! - Scan for connected Allwinner devices via USB
//! - Flash firmware to device storage (NAND/eMMC/SD card, etc.)
//! - Support multiple flash modes and post-flash actions
//! - Interactive TUI mode (default when no subcommand given)
//!
//! Usage examples:
//!   openixcli              # Launch interactive TUI (default)
//!   openixcli tui          # Launch interactive TUI (explicit)
//!   openixcli scan         # Scan for connected devices
//!   openixcli flash firmware.fex  # Flash firmware to device

use clap::Parser;

/// CLI structure parsed from command line arguments
use openixcli::cli::{Cli, Commands, UsbBackendArg};
use openixcli::commands::{self, parse_partition_list, FlashArgs};
use openixcli::convert::ConvertOptions;
use openixcli::tui;
use openixcli::utils::TermLogger;

/// Initialize the logging system
///
/// # Parameters
/// * `verbose` - Enable verbose output mode
///
/// If initialization fails, error message is printed to stderr but program continues
fn setup_logging(verbose: bool) {
    if let Err(e) = TermLogger::init(verbose) {
        eprintln!("Failed to initialize logger: {}", e);
    }
}

fn usb_backend(backend: UsbBackendArg) -> libefex::UsbBackend {
    match backend {
        UsbBackendArg::Auto => libefex::UsbBackend::Auto,
        UsbBackendArg::Libusb => libefex::UsbBackend::Libusb,
        UsbBackendArg::Winusb => libefex::UsbBackend::Winusb,
    }
}

fn configure_usb_backend(backend: UsbBackendArg) {
    if let Err(error) = libefex::Context::set_usb_backend_static(usb_backend(backend)) {
        eprintln!("Warning: failed to set USB backend: {:?}", error);
    }
}

#[tokio::main]
/// Program entry point
///
/// Parses command line arguments and executes corresponding commands:
/// - No subcommand / `tui`: Launch interactive TUI
/// - `scan`: Scan for USB devices
/// - `flash`: Flash firmware to device
///
/// # Returns
/// Ok(()) on success, anyhow::Error on failure
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Apply USB backend selection before any device access.
    // On Windows the default (Auto) resolves to WinUSB, which fails to open
    // some devices installed via Zadig/libwdi; --backend libusb works there.
    configure_usb_backend(cli.backend);

    match cli.command {
        None | Some(Commands::Tui) => {
            // TUI mode - don't init the standard logger, TUI has its own
            tui::run().await?;
        }
        Some(Commands::Scan { detailed }) => {
            setup_logging(cli.verbose);
            commands::scan::execute(detailed).await?;
        }
        Some(Commands::Flash {
            firmware,
            bus,
            port,
            verify,
            mode,
            partitions,
            post_action,
        }) => {
            setup_logging(cli.verbose);

            let args = FlashArgs {
                firmware_path: firmware.into(),
                bus,
                port,
                verify,
                mode,
                partitions: parse_partition_list(partitions),
                post_action,
                verbose: cli.verbose,
            };

            commands::flash::execute(args).await?;
        }
        Some(Commands::Inspect { firmware }) => {
            setup_logging(cli.verbose);
            commands::inspect::execute(firmware.into()).await?;
        }
        Some(Commands::Unpack { firmware, output }) => {
            setup_logging(cli.verbose);
            commands::unpack::execute(commands::UnpackArgs {
                firmware_path: firmware.into(),
                output: output.map(std::path::PathBuf::from),
            })
            .await?;
        }
        Some(Commands::Convert {
            firmware,
            output,
            target,
            logic_offset,
            uboot_start,
            nor_size,
            storage_size,
            secure,
        }) => {
            setup_logging(cli.verbose);
            commands::convert::execute(ConvertOptions {
                firmware_path: firmware.into(),
                output: output.map(std::path::PathBuf::from),
                target,
                logic_offset,
                uboot_start,
                nor_size,
                storage_size,
                secure,
            })
            .await?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usb_backend_mapping_covers_every_cli_value() {
        assert!(matches!(
            usb_backend(UsbBackendArg::Auto),
            libefex::UsbBackend::Auto
        ));
        assert!(matches!(
            usb_backend(UsbBackendArg::Libusb),
            libefex::UsbBackend::Libusb
        ));
        assert!(matches!(
            usb_backend(UsbBackendArg::Winusb),
            libefex::UsbBackend::Winusb
        ));
        configure_usb_backend(UsbBackendArg::Auto);
    }

    #[test]
    fn logging_setup_tolerates_repeated_initialization() {
        setup_logging(false);
        setup_logging(true);
    }
}
