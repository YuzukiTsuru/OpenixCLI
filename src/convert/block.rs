use std::fs::File;
use std::io::{Seek, SeekFrom, Write};

use log::{debug, info, warn};

use crate::firmware::sparse::{
    ChunkHeader, SparseHeader, CHUNK_TYPE_CRC32, CHUNK_TYPE_DONT_CARE, CHUNK_TYPE_FILL,
    CHUNK_TYPE_RAW,
};
use crate::firmware::OpenixPacker;

use super::gpt::{modify_backup, modify_primary, GPT_MIN_HEADER_SIZE, SECTOR_SIZE};
use super::{BlockConfig, PartitionEntry};

const BUFFER_SIZE: usize = 8 * 1024;
const GPT_SIZE: usize = 16 * 1024;
const MBR_SIZE: u64 = 64 * 1024;
const BOOT0_OFFSET: u64 = 16 * SECTOR_SIZE;
const BOOT0_BACKUP_OFFSET: u64 = 256 * SECTOR_SIZE;
const UBOOT_OFFSET: u64 = 24_576 * SECTOR_SIZE;
const UBOOT_BACKUP_OFFSET: u64 = 32_800 * SECTOR_SIZE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Layout {
    partition_start: u64,
    aligned_size: u64,
}

fn plan_layout(logic_offset: u64, partitions: &[PartitionEntry]) -> Result<Layout, String> {
    let partition_start = logic_offset
        .checked_add(MBR_SIZE)
        .ok_or_else(|| "Partition start offset overflows u64".to_string())?;
    let end = partitions
        .iter()
        .try_fold(partition_start, |offset, partition| {
            offset
                .checked_add(partition.size)
                .ok_or_else(|| format!("Partition {} range overflows u64", partition.name))
        })?;
    let aligned_size = end
        .checked_add(SECTOR_SIZE - 1)
        .map(|value| value / SECTOR_SIZE * SECTOR_SIZE)
        .ok_or_else(|| "Aligned firmware size overflows u64".to_string())?;
    Ok(Layout {
        partition_start,
        aligned_size,
    })
}

fn sparse_expanded_size(data: &[u8]) -> Result<Option<u64>, String> {
    let Some(header) = SparseHeader::parse(data) else {
        return Ok(None);
    };
    if !header.is_valid() {
        return Ok(None);
    }
    let block_size = u64::from(header.blk_sz);
    let total_blocks = u64::from(header.total_blks);
    block_size
        .checked_mul(total_blocks)
        .map(Some)
        .ok_or_else(|| "Sparse image expanded size overflows u64".to_string())
}

fn write_zeros(file: &mut File, length: u64) -> Result<(), String> {
    let zeros = [0u8; BUFFER_SIZE];
    let mut remaining = length;
    while remaining > 0 {
        let amount = remaining.min(zeros.len() as u64) as usize;
        file.write_all(&zeros[..amount])
            .map_err(|error| format!("Failed to write zero-filled sparse block: {error}"))?;
        remaining -= amount as u64;
    }
    Ok(())
}

fn write_fill(file: &mut File, pattern: [u8; 4], length: u64) -> Result<(), String> {
    let mut buffer = [0u8; BUFFER_SIZE];
    for chunk in buffer.chunks_exact_mut(4) {
        chunk.copy_from_slice(&pattern);
    }
    let mut remaining = length;
    while remaining > 0 {
        let amount = remaining.min(buffer.len() as u64) as usize;
        file.write_all(&buffer[..amount])
            .map_err(|error| format!("Failed to write sparse fill block: {error}"))?;
        remaining -= amount as u64;
    }
    Ok(())
}

fn decode_sparse(data: &[u8], file: &mut File) -> Result<u64, String> {
    let header = SparseHeader::parse(data)
        .filter(|header| header.is_valid())
        .ok_or_else(|| "Invalid Android sparse image header".to_string())?;
    let file_header_size = usize::from(header.file_hdr_sz);
    let chunk_header_size = usize::from(header.chunk_hdr_sz);
    let block_size = u64::from(header.blk_sz);
    let total_blocks = u64::from(header.total_blks);
    let total_chunks = header.total_chunks;
    let expected_total = block_size
        .checked_mul(total_blocks)
        .ok_or_else(|| "Sparse image expanded size overflows u64".to_string())?;

    let mut cursor = file_header_size;
    let mut written = 0u64;
    for index in 0..total_chunks {
        let header_end = cursor
            .checked_add(chunk_header_size)
            .ok_or_else(|| "Sparse chunk header offset overflows usize".to_string())?;
        let chunk_data = data
            .get(cursor..header_end)
            .ok_or_else(|| format!("Sparse chunk {index} header exceeds input"))?;
        let chunk = ChunkHeader::parse(chunk_data)
            .ok_or_else(|| format!("Sparse chunk {index} header is truncated"))?;
        let chunk_type = chunk.chunk_type;
        let chunk_blocks = u64::from(chunk.chunk_sz);
        let total_size = usize::try_from(chunk.total_sz)
            .map_err(|_| format!("Sparse chunk {index} size does not fit in memory"))?;
        if total_size < chunk_header_size {
            return Err(format!("Sparse chunk {index} total size is invalid"));
        }
        let payload_size = total_size - chunk_header_size;
        let payload_end = header_end
            .checked_add(payload_size)
            .ok_or_else(|| "Sparse chunk payload offset overflows usize".to_string())?;
        let payload = data
            .get(header_end..payload_end)
            .ok_or_else(|| format!("Sparse chunk {index} payload exceeds input"))?;
        let expanded = chunk_blocks
            .checked_mul(block_size)
            .ok_or_else(|| format!("Sparse chunk {index} expanded size overflows u64"))?;

        match chunk_type {
            CHUNK_TYPE_RAW => {
                if payload.len() as u64 != expanded {
                    return Err(format!(
                        "Sparse raw chunk {index} has an invalid payload size"
                    ));
                }
                file.write_all(payload)
                    .map_err(|error| format!("Failed to write sparse raw chunk: {error}"))?;
            }
            CHUNK_TYPE_FILL => {
                let pattern: [u8; 4] = payload
                    .try_into()
                    .map_err(|_| format!("Sparse fill chunk {index} must contain four bytes"))?;
                write_fill(file, pattern, expanded)?;
            }
            CHUNK_TYPE_DONT_CARE => {
                if !payload.is_empty() {
                    return Err(format!(
                        "Sparse skip chunk {index} has unexpected payload data"
                    ));
                }
                write_zeros(file, expanded)?;
            }
            CHUNK_TYPE_CRC32 => {
                if payload.len() != 4 || expanded != 0 {
                    return Err(format!("Sparse CRC chunk {index} is invalid"));
                }
            }
            other => return Err(format!("Unsupported sparse chunk type: 0x{other:04x}")),
        }
        written = written
            .checked_add(expanded)
            .ok_or_else(|| "Sparse output size overflows u64".to_string())?;
        cursor = payload_end;
    }

    if written != expected_total {
        return Err(format!(
            "Sparse output size mismatch: expected {expected_total}, wrote {written}"
        ));
    }
    Ok(written)
}

fn write_payload(file: &mut File, offset: u64, data: &[u8]) -> Result<u64, String> {
    file.seek(SeekFrom::Start(offset))
        .map_err(|error| format!("Failed to seek to offset {offset}: {error}"))?;
    if sparse_expanded_size(data)?.is_some() {
        decode_sparse(data, file)
    } else {
        file.write_all(data)
            .map_err(|error| format!("Failed to write at offset {offset}: {error}"))?;
        Ok(data.len() as u64)
    }
}

fn component_subtypes(flash_type: &str, secure: bool) -> (&'static str, &'static str) {
    if secure {
        let toc0 = match flash_type {
            "ufs" => "TOC0_UFS00000000",
            _ => "TOC0_SDCARD00000",
        };
        (toc0, "TOC1_00000000000")
    } else {
        let boot0 = match flash_type {
            "ufs" => "1234567890BUFS_0",
            "nand" => "BOOT0_0000000000",
            _ => "1234567890BOOT_0",
        };
        (boot0, "BOOTPKG-00000000")
    }
}

fn write_component_twice(
    packer: &mut OpenixPacker,
    file: &mut File,
    name: &str,
    subtype: &str,
    primary: u64,
    backup: u64,
) -> Result<(), String> {
    if packer.find_file_header_by_subtype(subtype).is_none() {
        warn!("{name} not found in firmware (subtype: {subtype})");
        return Ok(());
    }
    let data = packer
        .find_file_data_by_subtype(subtype)
        .map_err(|error| format!("Failed to extract {name}: {error}"))?;
    let written = write_payload(file, primary, &data)?;
    write_payload(file, backup, &data)?;
    info!(
        "{name} loaded: {written} bytes at sectors {} and {}",
        primary / SECTOR_SIZE,
        backup / SECTOR_SIZE
    );
    Ok(())
}

fn write_partition_table(
    packer: &mut OpenixPacker,
    file: &mut File,
    config: &BlockConfig,
    gpt_target_size: u64,
) -> Result<(), String> {
    if packer
        .find_file_header_by_subtype("1234567890___GPT")
        .is_some()
    {
        let data = packer
            .find_file_data_by_subtype("1234567890___GPT")
            .map_err(|error| format!("Failed to extract GPT: {error}"))?;
        let mut primary = data[..data.len().min(GPT_SIZE)].to_vec();
        if primary.len() >= GPT_MIN_HEADER_SIZE {
            match modify_primary(&mut primary, gpt_target_size) {
                Ok(last_lba) => info!("GPT last_usable_lba adjusted to {last_lba}"),
                Err(error) => warn!("GPT modification failed: {error}"),
            }
        } else {
            warn!("GPT data is too small: {} bytes", primary.len());
        }
        write_payload(file, 0, &primary)?;
        let mut backup = primary.clone();
        if let Err(error) = modify_backup(&mut backup, gpt_target_size, &primary) {
            warn!("Backup GPT modification failed: {error}");
        }
        write_payload(file, config.logic_offset, &backup)?;
    } else if packer
        .find_file_header_by_subtype("1234567890___MBR")
        .is_some()
    {
        let data = packer
            .find_file_data_by_subtype("1234567890___MBR")
            .map_err(|error| format!("Failed to extract MBR: {error}"))?;
        write_payload(
            file,
            config.logic_offset,
            &data[..data.len().min(MBR_SIZE as usize)],
        )?;
    } else {
        warn!("GPT/MBR not found in firmware");
    }
    Ok(())
}

pub(crate) fn merge(packer: &mut OpenixPacker, config: BlockConfig) -> Result<u64, String> {
    let layout = plan_layout(config.logic_offset, &config.partitions)?;
    let gpt_target_size = match config.storage_size {
        Some(size) if size >= layout.aligned_size => size,
        Some(size) => {
            warn!(
                "Specified storage size {size} is smaller than the firmware; using {}",
                layout.aligned_size
            );
            layout.aligned_size
        }
        None => layout.aligned_size,
    };

    let mut file = File::create(&config.output_path).map_err(|error| {
        format!(
            "Failed to create output file {}: {error}",
            config.output_path.display()
        )
    })?;
    file.set_len(layout.aligned_size)
        .map_err(|error| format!("Failed to size output file: {error}"))?;

    let (first_subtype, second_subtype) = component_subtypes(config.flash_type, config.is_secure);
    let (first_name, second_name) = if config.is_secure {
        ("TOC0", "TOC1")
    } else {
        ("Boot0", "BOOTPKG")
    };
    write_component_twice(
        packer,
        &mut file,
        first_name,
        first_subtype,
        BOOT0_OFFSET,
        BOOT0_BACKUP_OFFSET,
    )?;
    write_component_twice(
        packer,
        &mut file,
        second_name,
        second_subtype,
        UBOOT_OFFSET,
        UBOOT_BACKUP_OFFSET,
    )?;
    write_partition_table(packer, &mut file, &config, gpt_target_size)?;

    let mut partition_offset = layout.partition_start;
    for partition in &config.partitions {
        let partition_end = partition_offset
            .checked_add(partition.size)
            .ok_or_else(|| format!("Partition {} range overflows u64", partition.name))?;
        if partition.download_file.is_empty() {
            debug!("Partition {} has no download file", partition.name);
            partition_offset = partition_end;
            continue;
        }

        let subtype = packer.build_subtype_by_filename(&partition.download_file);
        if packer.find_file_header_by_subtype(&subtype).is_none() {
            warn!(
                "Partition file not found: {} -> {} ({})",
                partition.name, partition.download_file, subtype
            );
            partition_offset = partition_end;
            continue;
        }
        let data = packer
            .find_file_data_by_subtype(&subtype)
            .map_err(|error| format!("Failed to extract partition {}: {error}", partition.name))?;
        let expanded = sparse_expanded_size(&data)?.unwrap_or(data.len() as u64);
        if expanded > partition.size {
            return Err(format!(
                "Partition {} data size {expanded} exceeds partition size {}",
                partition.name, partition.size
            ));
        }
        let written = write_payload(&mut file, partition_offset, &data)?;
        info!(
            "Partition {} loaded: {written} bytes at sector {}",
            partition.name,
            partition_offset / SECTOR_SIZE
        );
        partition_offset = partition_end;
    }

    let current_size = file
        .metadata()
        .map_err(|error| format!("Failed to read output size: {error}"))?
        .len();
    let output_size = current_size
        .max(layout.aligned_size)
        .checked_add(SECTOR_SIZE - 1)
        .map(|value| value / SECTOR_SIZE * SECTOR_SIZE)
        .ok_or_else(|| "Output size alignment overflows u64".to_string())?;
    file.set_len(output_size)
        .map_err(|error| format!("Failed to finalize output size: {error}"))?;
    file.flush()
        .map_err(|error| format!("Failed to flush output file: {error}"))?;
    Ok(output_size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{temp_dir, write_struct};
    use std::fs;

    fn partition(name: &str, size: u64) -> PartitionEntry {
        PartitionEntry {
            name: name.to_string(),
            size,
            download_file: String::new(),
        }
    }

    #[test]
    fn layout_reserves_metadata_and_aligns_partition_data() {
        assert_eq!(
            plan_layout(1, &[partition("boot", 511)]).unwrap(),
            Layout {
                partition_start: MBR_SIZE + 1,
                aligned_size: MBR_SIZE + SECTOR_SIZE,
            }
        );
        assert!(plan_layout(u64::MAX, &[]).is_err());
    }

    #[test]
    fn component_subtypes_follow_secure_and_storage_modes() {
        assert_eq!(
            component_subtypes("emmc", false),
            ("1234567890BOOT_0", "BOOTPKG-00000000")
        );
        assert_eq!(
            component_subtypes("ufs", true),
            ("TOC0_UFS00000000", "TOC1_00000000000")
        );
    }

    #[test]
    fn short_or_non_sparse_data_has_no_expanded_size() {
        assert_eq!(sparse_expanded_size(&[]).unwrap(), None);
        assert_eq!(
            sparse_expanded_size(&[0; crate::firmware::sparse::SPARSE_HEADER_SIZE]).unwrap(),
            None
        );
    }

    #[test]
    fn decodes_android_sparse_raw_chunks() {
        let sparse_header = SparseHeader {
            magic: crate::firmware::sparse::SPARSE_HEADER_MAGIC,
            major_version: crate::firmware::sparse::SPARSE_HEADER_MAJOR_VER,
            minor_version: 0,
            file_hdr_sz: crate::firmware::sparse::SPARSE_HEADER_SIZE as u16,
            chunk_hdr_sz: crate::firmware::sparse::CHUNK_HEADER_SIZE as u16,
            blk_sz: 4096,
            total_blks: 1,
            total_chunks: 1,
            image_checksum: 0,
        };
        let chunk_header = ChunkHeader {
            chunk_type: CHUNK_TYPE_RAW,
            reserved: 0,
            chunk_sz: 1,
            total_sz: (crate::firmware::sparse::CHUNK_HEADER_SIZE + 4096) as u32,
        };
        let mut sparse = vec![0; crate::firmware::sparse::SPARSE_HEADER_SIZE];
        write_struct(&mut sparse, &sparse_header);
        let chunk_start = sparse.len();
        sparse.resize(chunk_start + crate::firmware::sparse::CHUNK_HEADER_SIZE, 0);
        write_struct(&mut sparse[chunk_start..], &chunk_header);
        sparse.extend(std::iter::repeat_n(0x5a, 4096));

        let directory = temp_dir("convert-sparse");
        let output = directory.path().join("raw.img");
        let mut file = File::create(&output).unwrap();
        assert_eq!(decode_sparse(&sparse, &mut file).unwrap(), 4096);
        file.flush().unwrap();
        assert_eq!(fs::read(output).unwrap(), vec![0x5a; 4096]);
    }
}
