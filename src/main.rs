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
use std::str::FromStr;

mod cli;
mod commands;
mod config;
mod firmware;
mod flash;
mod process;
mod tui;
mod utils;

/// CLI structure parsed from command line arguments
use cli::{Cli, Commands};
use commands::FlashArgs;
use utils::TermLogger;

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

            let flash_mode =
                commands::FlashMode::from_str(&mode).map_err(|e| anyhow::anyhow!("{}", e))?;

            let partition_list =
                partitions.map(|s| s.split(',').map(|p| p.trim().to_string()).collect());

            let args = FlashArgs {
                firmware_path: firmware.into(),
                bus,
                port,
                verify,
                mode: flash_mode,
                partitions: partition_list,
                post_action,
                verbose: cli.verbose,
            };

            commands::flash::execute(args).await?;
        }
    }

    Ok(())
}
