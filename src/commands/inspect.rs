//! Inspect command implementation
//!
//! Parses a firmware file and displays its contents: image header, embedded
//! file list, and MBR partition table. Read-only, no device required.

use std::path::PathBuf;

use colored::Colorize;

use crate::config::mbr_parser::SunxiMbr;
use crate::firmware::sparse::{is_sparse_format, SPARSE_HEADER_SIZE};
use crate::firmware::{LoadedFirmware, OpenixPacker};

/// Execute the inspect command
///
/// Loads the firmware and prints its image header, embedded file list
/// (with sparse detection), and MBR partition table.
///
/// # Arguments
/// * `firmware` - Path to the firmware file
///
/// # Returns
/// Ok(()) on success, Error on failure
pub async fn execute(firmware: PathBuf) -> anyhow::Result<()> {
    if !firmware.exists() {
        eprintln!(
            "{}",
            format!("Firmware file not found: {}", firmware.display()).red()
        );
        return Err(anyhow::anyhow!("Firmware file not found"));
    }

    println!(
        "{} {}",
        "Inspecting firmware:".cyan().bold(),
        firmware.display()
    );

    let loaded = LoadedFirmware::load(&firmware)?;
    let info = loaded.image_info().clone();
    let mut packer = loaded.into_packer();

    // Copy header fields out first: ImageHeader is #[repr(C, packed)], so
    // borrowing its fields (as println! does) would be unaligned UB (E0793).
    let magic = info.header.magic;
    let header_version = info.header.header_version;
    let ram_base = info.header.ram_base;
    let version = info.header.version;

    // ---- image header ----
    println!();
    println!("{}", "Image Header".cyan().bold());
    println!("  Magic           : {}", String::from_utf8_lossy(&magic));
    println!("  Header version  : 0x{:08x}", header_version);
    println!(
        "  Image size      : {} ({} bytes)",
        human_size(info.image_size),
        info.image_size
    );
    println!("  Embedded files  : {}", info.num_files);
    println!("  RAM base        : 0x{:08x}", ram_base);
    println!("  Version         : 0x{:08x}", version);
    println!("  Encrypted       : {}", info.is_encrypted);

    // ---- embedded files ----
    println!();
    println!(
        "{} ({} entries)",
        "Embedded Files".cyan().bold(),
        info.files.len()
    );
    println!(
        "  {:<34} {:<10} {:>12} {:>12}",
        "filename", "type", "size", "offset"
    );
    for f in &info.files {
        let sparse = probe_sparse(&mut packer, &f.maintype, &f.subtype);
        let tag = if sparse {
            " [sparse]".yellow().to_string()
        } else {
            String::new()
        };
        println!(
            "  {:<34} {:<10} {:>12} {:>12}{}",
            truncate(&f.filename, 34),
            truncate(&f.maintype, 10),
            human_size(f.original_length),
            f.offset,
            tag
        );
    }

    // ---- MBR partitions ----
    println!();
    println!("{}", "MBR Partitions".cyan().bold());
    match packer.get_mbr() {
        Ok(mbr_data) => match SunxiMbr::parse(&mbr_data) {
            Ok(mbr) => {
                println!("  Partitions: {}, magic: {}", mbr.part_count, mbr.magic);
                println!(
                    "  {:<16} {:<12} {:>14} {:>14} {:<4}",
                    "name", "class", "address", "length", "ro"
                );
                for p in &mbr.partitions {
                    println!(
                        "  {:<16} {:<12} {:>14} {:>14} {}",
                        truncate(&p.name, 16),
                        truncate(&p.classname, 12),
                        p.address(),
                        p.length(),
                        if p.readonly() { "ro" } else { "rw" }
                    );
                }
            }
            Err(e) => println!("  {}", format!("Failed to parse MBR: {}", e).yellow()),
        },
        Err(e) => println!(
            "  {}",
            format!("No MBR in firmware: {}", e).yellow()
        ),
    }

    Ok(())
}

/// Probe whether a firmware entry is Android sparse format by reading its
/// first `SPARSE_HEADER_SIZE` bytes.
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
