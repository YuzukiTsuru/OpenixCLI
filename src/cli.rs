//! Command-line interface definitions
//!
//! Defines the CLI structure using clap for argument parsing

use clap::{Parser, Subcommand, ValueEnum};

use crate::flash::{FlashMode, PostAction};

/// Main CLI structure
///
/// # Fields
/// * `command` - The subcommand to execute (scan, flash, or tui). Defaults to TUI if none given.
/// * `verbose` - Enable verbose output
#[derive(Parser)]
#[command(name = "openixcli")]
#[command(about = "Firmware flashing CLI tool for Allwinner chips", long_about = None)]
#[command(version)]
pub struct Cli {
    /// The subcommand to execute (defaults to TUI if omitted)
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Enable verbose output
    #[arg(short, long, global = true, help = "Enable verbose output")]
    pub verbose: bool,

    /// USB backend selection (Windows: winusb default; use libusb if open fails)
    #[arg(
        long,
        global = true,
        value_enum,
        default_value_t = UsbBackendArg::Auto,
        help = "USB backend: auto, libusb, winusb"
    )]
    pub backend: UsbBackendArg,
}

/// USB backend choice exposed on the command line
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum UsbBackendArg {
    /// Platform default (Windows: winusb)
    Auto,
    /// Force libusb backend
    Libusb,
    /// Force winusb backend (Windows only)
    Winusb,
}

/// Available CLI commands
#[derive(Subcommand)]
pub enum Commands {
    /// Scan for connected devices
    Scan {
        /// Get detailed device information (requires device initialization)
        #[arg(short = 'l', long, help = "Get detailed device information")]
        detailed: bool,
    },

    /// Flash firmware to device
    Flash {
        /// Path to firmware file
        #[arg(help = "Path to firmware file")]
        firmware: String,

        /// USB bus number
        #[arg(short, long, help = "USB bus number")]
        bus: Option<u8>,

        /// USB port number
        #[arg(short = 'P', long, help = "USB port number")]
        port: Option<u8>,

        /// Enable verification after write
        #[arg(
            short = 'V',
            long,
            default_value_t = true,
            action = clap::ArgAction::Set,
            help = "Enable verification after write"
        )]
        verify: bool,

        /// Flash mode
        /// - partition: Flash only specified partitions
        /// - keep_data: Keep existing data
        /// - partition_erase: Erase partitions before flashing
        /// - full_erase: Erase all data before flashing
        #[arg(
            short,
            long,
            default_value = "full_erase",
            help = "Flash mode: partition, keep_data, partition_erase, full_erase"
        )]
        mode: FlashMode,

        /// Partitions to flash (comma-separated)
        #[arg(short = 'p', long, help = "Partitions to flash (comma-separated)")]
        partitions: Option<String>,

        /// Post-flash action
        /// - reboot: Reboot device after flashing
        /// - poweroff: Power off device after flashing
        /// - shutdown: Shutdown device after flashing
        #[arg(
            short = 'a',
            long,
            default_value = "reboot",
            help = "Post-flash action: reboot, poweroff, shutdown"
        )]
        post_action: PostAction,
    },

    /// Inspect firmware contents (image header, embedded files, MBR partitions)
    Inspect {
        /// Path to firmware file
        #[arg(help = "Path to firmware file")]
        firmware: String,
    },

    /// Unpack firmware data to disk (embedded files + partition images)
    Unpack {
        /// Path to firmware file
        #[arg(help = "Path to firmware file")]
        firmware: String,

        /// Output directory (default: ./<firmware>_unpacked)
        #[arg(short, long, help = "Output directory")]
        output: Option<String>,
    },

    /// Launch interactive TUI mode
    Tui,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_tui_behavior_with_auto_backend() {
        let cli = Cli::try_parse_from(["openixcli"]).unwrap();
        assert!(cli.command.is_none());
        assert!(!cli.verbose);
        assert_eq!(cli.backend, UsbBackendArg::Auto);

        let cli = Cli::try_parse_from(["openixcli", "--backend", "libusb", "tui"]).unwrap();
        assert_eq!(cli.backend, UsbBackendArg::Libusb);
        assert!(matches!(cli.command, Some(Commands::Tui)));
    }

    #[test]
    fn parses_scan_inspect_and_unpack_commands() {
        let scan = Cli::try_parse_from(["openixcli", "-v", "scan", "--detailed"]).unwrap();
        assert!(scan.verbose);
        assert!(matches!(
            scan.command,
            Some(Commands::Scan { detailed: true })
        ));

        let inspect = Cli::try_parse_from(["openixcli", "inspect", "firmware.fex"]).unwrap();
        assert!(matches!(
            inspect.command,
            Some(Commands::Inspect { firmware }) if firmware == "firmware.fex"
        ));

        let unpack =
            Cli::try_parse_from(["openixcli", "unpack", "firmware.fex", "--output", "out"])
                .unwrap();
        assert!(matches!(
            unpack.command,
            Some(Commands::Unpack { firmware, output: Some(output) })
                if firmware == "firmware.fex" && output == "out"
        ));
    }

    #[test]
    fn parses_all_flash_options_and_defaults() {
        let defaults = Cli::try_parse_from(["openixcli", "flash", "firmware.fex"]).unwrap();
        assert!(matches!(
            defaults.command,
            Some(Commands::Flash {
                verify: true,
                mode: FlashMode::FullErase,
                post_action: PostAction::Reboot,
                ..
            })
        ));

        let cli = Cli::try_parse_from([
            "openixcli",
            "--backend",
            "winusb",
            "flash",
            "firmware.fex",
            "--bus",
            "2",
            "--port",
            "7",
            "--verify=false",
            "--mode",
            "partition",
            "--partitions",
            "boot,system",
            "--post-action",
            "shutdown",
        ])
        .unwrap();
        assert_eq!(cli.backend, UsbBackendArg::Winusb);
        assert!(matches!(
            cli.command,
            Some(Commands::Flash {
                firmware,
                bus: Some(2),
                port: Some(7),
                verify: false,
                mode: FlashMode::Partition,
                partitions: Some(partitions),
                post_action: PostAction::Shutdown,
            }) if firmware == "firmware.fex" && partitions == "boot,system"
        ));
    }

    #[test]
    fn rejects_invalid_values_and_missing_required_arguments() {
        assert!(Cli::try_parse_from(["openixcli", "--backend", "invalid"]).is_err());
        assert!(Cli::try_parse_from(["openixcli", "flash"]).is_err());
        assert!(
            Cli::try_parse_from(["openixcli", "flash", "firmware.fex", "--mode", "invalid"])
                .is_err()
        );
        assert!(Cli::try_parse_from(["openixcli", "unknown"]).is_err());
    }
}
