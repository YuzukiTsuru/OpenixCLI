use clap::Parser;
use std::str::FromStr;

mod cli;
mod commands;
mod config;
mod firmware;
mod flash;
mod process;
mod utils;

use cli::{Cli, Commands};
use commands::FlashArgs;
use utils::TermLogger;

fn setup_logging(verbose: bool) {
    if let Err(e) = TermLogger::init(verbose) {
        eprintln!("Failed to initialize logger: {}", e);
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    setup_logging(cli.verbose);

    match cli.command {
        Commands::Scan => {
            commands::scan::execute().await?;
        }
        Commands::Flash {
            firmware,
            bus,
            port,
            verify,
            mode,
            partitions,
            post_action,
        } => {
            let flash_mode = commands::FlashMode::from_str(&mode)
                .map_err(|e| anyhow::anyhow!("{}", e))?;

            let partition_list = partitions
                .map(|s| s.split(',').map(|p| p.trim().to_string()).collect());

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
