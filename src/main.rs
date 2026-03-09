use clap::Parser;
use colored::Colorize;
use env_logger::{Builder, Env};

mod cli;
mod commands;
mod config;
mod firmware;
mod flash;
mod utils;

use cli::{Cli, Commands};

fn setup_logging(verbose: bool) {
    let level = if verbose { "debug" } else { "info" };
    Builder::from_env(Env::default().default_filter_or(level))
        .format(|buf, record| {
            use std::io::Write;
            let level_style = match record.level() {
                log::Level::Error => "ERRO".red().bold(),
                log::Level::Warn => "WARN".yellow().bold(),
                log::Level::Info => "INFO".green().bold(),
                log::Level::Debug => "DEBG".blue().bold(),
                log::Level::Trace => "TRCE".white().bold(),
            };
            writeln!(buf, "[{}] {}", level_style, record.args())
        })
        .init();
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
