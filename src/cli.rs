//! Command-line interface definitions
//!
//! Defines the CLI structure using clap for argument parsing

use clap::{Parser, Subcommand, ValueEnum};

pub use crate::convert::{ConvertTarget, SecureMode};
use crate::flash::{FlashMode, PostAction};
pub use crate::raw::{RawMode, RawStorage};

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

    /// Convert an IMAGEWTY firmware package to a raw programmer image
    Convert {
        /// Path to the IMAGEWTY firmware file
        #[arg(help = "Path to firmware file")]
        firmware: String,

        /// Output image path (default depends on target)
        #[arg(short, long, help = "Output raw image path")]
        output: Option<String>,

        /// Target storage layout
        #[arg(short, long, value_enum, default_value_t = ConvertTarget::Emmc)]
        target: ConvertTarget,

        /// Logical partition offset in 512-byte sectors (auto-detected when omitted)
        #[arg(long)]
        logic_offset: Option<u64>,

        /// U-Boot offset in 512-byte sectors for SPI NOR (auto-detected when omitted)
        #[arg(long)]
        uboot_start: Option<u64>,

        /// SPI NOR capacity in MiB
        #[arg(long, default_value_t = 16)]
        nor_size: u64,

        /// GPT target storage size, for example auto, 8GB, or 512MB
        #[arg(long, default_value = "auto")]
        storage_size: String,

        /// Secure boot component mode
        #[arg(long, value_enum, default_value_t = SecureMode::Auto)]
        secure: SecureMode,
    },

    /// Flash a raw disk image using an IMAGEWTY boot firmware
    Raw {
        /// Path to the IMAGEWTY boot firmware
        #[arg(help = "Path to boot firmware file")]
        firmware: String,

        /// Path to the raw disk image
        #[arg(help = "Path to raw image file")]
        image: String,

        /// Raw flashing mode
        #[arg(long, value_enum, default_value_t = RawMode::LogicOffset)]
        mode: RawMode,

        /// Target storage used to select the default logical offset
        #[arg(long, value_enum, default_value_t = RawStorage::Sdmmc)]
        storage: RawStorage,

        /// Logical offset in 512-byte sectors
        #[arg(long)]
        logic_offset: Option<u64>,

        /// USB bus number
        #[arg(short, long)]
        bus: Option<u8>,

        /// USB port number
        #[arg(short = 'P', long)]
        port: Option<u8>,
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
    fn parses_convert_defaults_and_explicit_options() {
        let defaults = Cli::try_parse_from(["openixcli", "convert", "firmware.img"]).unwrap();
        assert!(matches!(
            defaults.command,
            Some(Commands::Convert {
                firmware,
                output: None,
                target: ConvertTarget::Emmc,
                logic_offset: None,
                uboot_start: None,
                nor_size: 16,
                storage_size,
                secure: SecureMode::Auto,
            }) if firmware == "firmware.img" && storage_size == "auto"
        ));

        let explicit = Cli::try_parse_from([
            "openixcli",
            "convert",
            "firmware.img",
            "--output",
            "raw.img",
            "--target",
            "spinor",
            "--logic-offset",
            "2048",
            "--uboot-start",
            "64",
            "--nor-size",
            "32",
            "--storage-size",
            "8GB",
            "--secure",
            "enabled",
        ])
        .unwrap();
        assert!(matches!(
            explicit.command,
            Some(Commands::Convert {
                firmware,
                output: Some(output),
                target: ConvertTarget::Spinor,
                logic_offset: Some(2048),
                uboot_start: Some(64),
                nor_size: 32,
                storage_size,
                secure: SecureMode::Enabled,
            }) if firmware == "firmware.img" && output == "raw.img" && storage_size == "8GB"
        ));
    }

    #[test]
    fn parses_raw_defaults_and_both_modes() {
        let defaults = Cli::try_parse_from(["openixcli", "raw", "boot.img", "disk.img"]).unwrap();
        assert!(matches!(
            defaults.command,
            Some(Commands::Raw {
                firmware,
                image,
                mode: RawMode::LogicOffset,
                storage: RawStorage::Sdmmc,
                logic_offset: None,
                bus: None,
                port: None,
            }) if firmware == "boot.img" && image == "disk.img"
        ));

        let command = Cli::try_parse_from([
            "openixcli",
            "raw",
            "boot.img",
            "disk.img",
            "--mode",
            "command",
            "--storage",
            "nor",
            "--logic-offset",
            "2106",
            "--bus",
            "2",
            "--port",
            "7",
        ])
        .unwrap();
        assert!(matches!(
            command.command,
            Some(Commands::Raw {
                mode: RawMode::Command,
                storage: RawStorage::Nor,
                logic_offset: Some(2106),
                bus: Some(2),
                port: Some(7),
                ..
            })
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
        assert!(Cli::try_parse_from(["openixcli", "raw"]).is_err());
        assert!(
            Cli::try_parse_from(["openixcli", "flash", "firmware.fex", "--mode", "invalid"])
                .is_err()
        );
        assert!(Cli::try_parse_from(["openixcli", "unknown"]).is_err());
    }
}
