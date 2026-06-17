//! Unpack command implementation
//!
//! Extracts firmware data to disk: every embedded file into `<out>/files/`
//! and every MBR partition image into `<out>/partitions/`. Sparse partition
//! images are written as-is (not expanded to raw).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use colored::Colorize;

use crate::commands::UnpackArgs;
use crate::config::mbr_parser::SunxiMbr;
use crate::config::partition::OpenixPartition;
use crate::firmware::sparse::{is_sparse_format, SPARSE_HEADER_SIZE};
use crate::firmware::{LoadedFirmware, OpenixPacker};

/// Maintype under which partition image data is stored in IMAGEWTY firmware.
const MAINTYPE_RFSFAT16: &str = "RFSFAT16";
/// Fallback maintype used by some firmware variants.
const MAINTYPE_FALLBACK: &str = "12345678";
/// Chunk size for streamed extraction (avoids allocating the whole image).
const CHUNK_SIZE: u64 = 4 * 1024 * 1024;

/// Execute the unpack command
///
/// Loads the firmware and extracts every embedded file into `<out>/files/`
/// and every MBR partition image into `<out>/partitions/`.
///
/// # Arguments
/// * `args` - Unpack arguments including firmware path and optional output dir
///
/// # Returns
/// Ok(()) on success, Error on failure
pub async fn execute(args: UnpackArgs) -> anyhow::Result<()> {
    if !args.firmware_path.exists() {
        eprintln!(
            "{}",
            format!("Firmware file not found: {}", args.firmware_path.display()).red()
        );
        return Err(anyhow::anyhow!("Firmware file not found"));
    }

    let outdir = args
        .output
        .unwrap_or_else(|| default_outdir(&args.firmware_path));
    let files_dir = outdir.join("files");
    let parts_dir = outdir.join("partitions");
    fs::create_dir_all(&files_dir)?;
    fs::create_dir_all(&parts_dir)?;

    println!(
        "{} {} -> {}",
        "Unpacking firmware:".cyan().bold(),
        args.firmware_path.display(),
        outdir.display()
    );

    let loaded = LoadedFirmware::load(&args.firmware_path)?;
    let info = loaded.image_info().clone();
    let mut packer = loaded.into_packer();

    // ---- embedded files view ----
    println!();
    println!(
        "{} ({} entries)",
        "Embedded files".cyan().bold(),
        info.files.len()
    );
    let mut n_files = 0u64;
    for f in &info.files {
        let name = sanitize_filename(&f.filename);
        let file_name = if name.is_empty() {
            "unnamed.bin".to_string()
        } else {
            name
        };
        let out_path = files_dir.join(&file_name);
        match extract_range(
            &mut packer,
            &f.maintype,
            &f.subtype,
            f.original_length,
            &out_path,
        ) {
            Ok(n) => {
                let tag = if probe_sparse(&mut packer, &f.maintype, &f.subtype) {
                    " [sparse]"
                } else {
                    ""
                };
                println!(
                    "  {} {:<34} {} ({} bytes){}",
                    "+".green(),
                    truncate(&f.filename, 34),
                    human_size(n),
                    n,
                    tag.yellow()
                );
                n_files += 1;
            }
            Err(e) => {
                eprintln!("  {} {} ({})", "skip".red(), f.filename, e);
            }
        }
    }

    // ---- partitions view ----
    println!();
    println!("{}", "Partitions".cyan().bold());

    let mut cfg = OpenixPartition::new();
    if let Ok(data) = packer.get_sys_partition() {
        cfg.parse_from_data(&data);
    }
    let configs = cfg.get_partitions();

    let mut n_parts = 0u64;
    match packer.get_mbr() {
        Ok(mbr_data) => match SunxiMbr::parse(&mbr_data) {
            Ok(mbr) => {
                for p in &mbr.partitions {
                    let Some(dl) = configs
                        .iter()
                        .find(|c| c.name == p.name)
                        .map(|c| c.downloadfile.clone())
                        .filter(|s| !s.is_empty())
                    else {
                        eprintln!("  {} {} (no download file)", "skip".red(), p.name);
                        continue;
                    };

                    let subtype = packer.build_subtype_by_filename(&dl);
                    let Some((maintype, length)) = resolve_info(&packer, &subtype) else {
                        eprintln!("  {} {} (image not found)", "skip".red(), p.name);
                        continue;
                    };

                    let out_path =
                        parts_dir.join(format!("{}.img", sanitize_filename(&p.name)));
                    match extract_range(&mut packer, maintype, &subtype, length, &out_path) {
                        Ok(n) => {
                            let tag = if probe_sparse(&mut packer, maintype, &subtype) {
                                " [sparse]"
                            } else {
                                ""
                            };
                            println!(
                                "  {} {:<16} {} ({} bytes){}",
                                "+".green(),
                                truncate(&p.name, 16),
                                human_size(n),
                                n,
                                tag.yellow()
                            );
                            n_parts += 1;
                        }
                        Err(e) => eprintln!("  {} {} ({})", "fail".red(), p.name, e),
                    }
                }
            }
            Err(e) => eprintln!("  {}", format!("Failed to parse MBR: {}", e).yellow()),
        },
        Err(e) => eprintln!(
            "  {}",
            format!("No MBR in firmware: {}", e).yellow()
        ),
    }

    println!();
    println!(
        "{} extracted {} file(s), {} partition(s) -> {}",
        "Done:".green().bold(),
        n_files,
        n_parts,
        outdir.display()
    );

    Ok(())
}

/// Resolve `(maintype, length)` for a partition by subtype, trying RFSFAT16
/// then the fallback maintype (mirrors the flash download path).
fn resolve_info(packer: &OpenixPacker, subtype: &str) -> Option<(&'static str, u64)> {
    if let Some((_, len)) = packer.get_file_info_by_maintype_subtype(MAINTYPE_RFSFAT16, subtype) {
        return Some((MAINTYPE_RFSFAT16, len));
    }
    if let Some((_, len)) = packer.get_file_info_by_maintype_subtype(MAINTYPE_FALLBACK, subtype) {
        return Some((MAINTYPE_FALLBACK, len));
    }
    None
}

/// Stream a firmware entry to `out_path` in fixed-size chunks.
fn extract_range(
    packer: &mut OpenixPacker,
    maintype: &str,
    subtype: &str,
    total: u64,
    out_path: &Path,
) -> anyhow::Result<u64> {
    let mut file = fs::File::create(out_path)?;
    if total == 0 {
        file.flush()?;
        return Ok(0);
    }
    let mut written = 0u64;
    while written < total {
        let len = std::cmp::min(CHUNK_SIZE, total - written);
        let data =
            packer.get_file_data_range_by_maintype_subtype(maintype, subtype, written, len)?;
        file.write_all(&data)?;
        written += len;
    }
    file.flush()?;
    Ok(written)
}

fn probe_sparse(packer: &mut OpenixPacker, maintype: &str, subtype: &str) -> bool {
    match packer.get_file_data_range_by_maintype_subtype(
        maintype,
        subtype,
        0,
        SPARSE_HEADER_SIZE as u64,
    ) {
        Ok(head) => is_sparse_format(&head),
        Err(_) => false,
    }
}

fn default_outdir(firmware: &Path) -> PathBuf {
    let stem = firmware
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("firmware");
    PathBuf::from(format!("{}_unpacked", stem))
}

/// Make a firmware-internal filename safe for use as a filesystem path.
fn sanitize_filename(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | ' ' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    cleaned
        .trim_matches(|c: char| c == '.' || c.is_whitespace())
        .to_string()
}

fn human_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

fn truncate(s: &str, n: usize) -> String {
    if n == 0 {
        return String::new();
    }
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(n.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}
