//! Raw programmer image conversion command.

use colored::Colorize;

use crate::convert::{self, ConvertOptions};

pub async fn execute(options: ConvertOptions) -> anyhow::Result<()> {
    let result = tokio::task::spawn_blocking(move || convert::convert(options))
        .await
        .map_err(|error| anyhow::anyhow!("Conversion task failed: {error}"))?
        .map_err(anyhow::Error::msg)?;

    println!(
        "{} {} bytes -> {}",
        "Converted firmware:".green().bold(),
        result.output_size,
        result.output_path.display()
    );
    Ok(())
}
