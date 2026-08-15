use std::fs::File;
use std::io::Write;
use std::ops::Range;

use log::{debug, info, warn};

use crate::firmware::OpenixPacker;

use super::SpinorConfig;

const GPT_SIZE: u64 = 16 * 1024;

fn checked_range(
    offset: u64,
    length: u64,
    capacity: u64,
    component: &str,
) -> Result<Range<usize>, String> {
    let end = offset
        .checked_add(length)
        .ok_or_else(|| format!("{component} range overflows u64"))?;
    if end > capacity {
        return Err(format!("{component} exceeds NOR size"));
    }
    Ok(
        usize::try_from(offset).map_err(|_| format!("{component} offset does not fit in memory"))?
            ..usize::try_from(end)
                .map_err(|_| format!("{component} end offset does not fit in memory"))?,
    )
}

fn copy_component(
    packer: &mut OpenixPacker,
    output: &mut [u8],
    subtype: &str,
    offset: u64,
    capacity: u64,
    name: &str,
) -> Result<(), String> {
    if packer.find_file_header_by_subtype(subtype).is_none() {
        warn!("{name} not found in firmware (subtype: {subtype})");
        return Ok(());
    }
    let data = packer
        .find_file_data_by_subtype(subtype)
        .map_err(|error| format!("Failed to extract {name}: {error}"))?;
    let range = checked_range(offset, data.len() as u64, capacity, name)?;
    output[range].copy_from_slice(&data);
    info!("{name} loaded: {} bytes at offset {offset}", data.len());
    Ok(())
}

pub(crate) fn merge(packer: &mut OpenixPacker, config: SpinorConfig) -> Result<u64, String> {
    if config.nor_size == 0 {
        return Err("NOR size must be greater than zero".to_string());
    }

    let mut partition_offset = config
        .logic_start
        .checked_add(GPT_SIZE)
        .ok_or_else(|| "Partition start offset overflows u64".to_string())?;
    for partition in &config.partitions {
        partition_offset = partition_offset
            .checked_add(partition.size)
            .ok_or_else(|| format!("Partition {} range overflows u64", partition.name))?;
        if partition_offset > config.nor_size {
            return Err(format!("Partition {} exceeds NOR size", partition.name));
        }
    }

    let capacity = usize::try_from(config.nor_size)
        .map_err(|_| "NOR size does not fit in memory".to_string())?;
    let mut output = Vec::new();
    output
        .try_reserve_exact(capacity)
        .map_err(|error| format!("Failed to allocate NOR output buffer: {error}"))?;
    output.resize(capacity, 0xff);

    copy_component(
        packer,
        &mut output,
        "1234567890BNOR_0",
        0,
        config.nor_size,
        "Boot0",
    )?;
    copy_component(
        packer,
        &mut output,
        "BOOTPKG-NOR00000",
        config.uboot_start,
        config.nor_size,
        "U-Boot",
    )?;
    copy_component(
        packer,
        &mut output,
        "1234567890___GPT",
        config.logic_start,
        config.nor_size,
        "GPT",
    )?;

    let mut partition_offset = config.logic_start + GPT_SIZE;
    for partition in &config.partitions {
        let partition_end = partition_offset + partition.size;
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
        if data.len() as u64 > partition.size {
            return Err(format!(
                "Partition {} file size exceeds partition size",
                partition.name
            ));
        }
        let range = checked_range(
            partition_offset,
            data.len() as u64,
            config.nor_size,
            &format!("Partition {}", partition.name),
        )?;
        output[range].copy_from_slice(&data);
        info!(
            "Partition {} loaded: {} bytes at offset {}",
            partition.name,
            data.len(),
            partition_offset
        );
        partition_offset = partition_end;
    }

    let mut file = File::create(&config.output_path).map_err(|error| {
        format!(
            "Failed to create output file {}: {error}",
            config.output_path.display()
        )
    })?;
    file.write_all(&output)
        .map_err(|error| format!("Failed to write output file: {error}"))?;
    file.flush()
        .map_err(|error| format!("Failed to flush output file: {error}"))?;
    Ok(config.nor_size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_ranges_accept_exact_end_and_reject_overflow() {
        assert_eq!(checked_range(8, 2, 10, "item").unwrap(), 8..10);
        assert_eq!(
            checked_range(9, 2, 10, "item").unwrap_err(),
            "item exceeds NOR size"
        );
        assert_eq!(
            checked_range(u64::MAX, 1, u64::MAX, "item").unwrap_err(),
            "item range overflows u64"
        );
    }
}
