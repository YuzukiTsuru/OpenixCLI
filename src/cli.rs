use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "openixcli")]
#[command(about = "Firmware flashing CLI tool for Allwinner chips", long_about = None)]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    #[arg(short, long, global = true, help = "Enable verbose output")]
    pub verbose: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    #[command(about = "Scan for connected devices")]
    Scan,

    #[command(about = "Flash firmware to device")]
    Flash {
        #[arg(help = "Path to firmware file")]
        firmware: String,

        #[arg(short, long, help = "USB bus number")]
        bus: Option<u8>,

        #[arg(short = 'P', long, help = "USB port number")]
        port: Option<u8>,

        #[arg(
            short = 'V',
            long,
            default_value = "true",
            help = "Enable verification after write"
        )]
        verify: bool,

        #[arg(
            short,
            long,
            default_value = "full_erase",
            help = "Flash mode: partition, keep_data, partition_erase, full_erase"
        )]
        mode: String,

        #[arg(short = 'p', long, help = "Partitions to flash (comma-separated)")]
        partitions: Option<String>,

        #[arg(
            short = 'a',
            long,
            default_value = "reboot",
            help = "Post-flash action: reboot, poweroff, shutdown"
        )]
        post_action: String,
    },
}
