use clap::Parser;

mod cli;
mod commands;
mod config;
mod firmware;
mod flash;
mod process;
mod utils;

use cli::{Cli, Commands};
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
            commands::flash::execute(
                &firmware,
                bus,
                port,
                verify,
                &mode,
                partitions.as_deref(),
                &post_action,
                cli.verbose,
            )
            .await?;
        }
    }

    Ok(())
}
