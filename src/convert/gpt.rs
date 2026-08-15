//! Minimal GPT header rewriting used by raw image conversion.

use std::mem::size_of;

pub(crate) const SECTOR_SIZE: u64 = 512;
const GPT_SIGNATURE: &[u8; 8] = b"EFI PART";
const GPT_HEADER_SIZE: usize = 92;
pub(crate) const GPT_MIN_HEADER_SIZE: usize = SECTOR_SIZE as usize + GPT_HEADER_SIZE;

#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
struct GptHeaderRaw {
    signature: [u8; 8],
    revision: u32,
    header_size: u32,
    header_crc32: u32,
    reserved: u32,
    my_lba: u64,
    alternate_lba: u64,
    first_usable_lba: u64,
    last_usable_lba: u64,
    disk_guid: [u8; 16],
    partition_entry_lba: u64,
    num_partition_entries: u32,
    size_of_partition_entry: u32,
    partition_entry_crc32: u32,
}

impl GptHeaderRaw {
    fn parse(data: &[u8]) -> Result<Self, String> {
        if data.len() < GPT_MIN_HEADER_SIZE {
            return Err(format!("GPT data is too small: {} bytes", data.len()));
        }
        let mut raw = Self::default();
        unsafe {
            std::ptr::copy_nonoverlapping(
                data[SECTOR_SIZE as usize..].as_ptr(),
                &mut raw as *mut Self as *mut u8,
                size_of::<Self>(),
            );
        }
        if &raw.signature != GPT_SIGNATURE {
            return Err("Invalid GPT signature".to_string());
        }
        if raw.header_size < GPT_HEADER_SIZE as u32 || raw.header_size as usize > GPT_HEADER_SIZE {
            let header_size = raw.header_size;
            return Err(format!("Invalid GPT header size: {header_size}"));
        }
        Ok(raw)
    }

    fn write_at_header_offset(&self, data: &mut [u8]) -> Result<(), String> {
        if data.len() < GPT_MIN_HEADER_SIZE {
            return Err(format!("GPT data is too small: {} bytes", data.len()));
        }
        unsafe {
            std::ptr::copy_nonoverlapping(
                self as *const Self as *const u8,
                data[SECTOR_SIZE as usize..].as_mut_ptr(),
                size_of::<Self>(),
            );
        }
        Ok(())
    }

    fn update_crc32(&mut self) {
        self.header_crc32 = 0;
        let bytes = unsafe {
            std::slice::from_raw_parts(self as *const Self as *const u8, size_of::<Self>())
        };
        self.header_crc32 = crc32fast::hash(bytes);
    }

    fn partition_entry_sectors(&self) -> Result<u64, String> {
        let bytes = u64::from(self.num_partition_entries)
            .checked_mul(u64::from(self.size_of_partition_entry))
            .ok_or_else(|| "GPT partition entry size overflows u64".to_string())?;
        Ok(bytes.div_ceil(SECTOR_SIZE))
    }
}

fn target_lbas(header: &GptHeaderRaw, total_size_bytes: u64) -> Result<(u64, u64), String> {
    let total_sectors = total_size_bytes / SECTOR_SIZE;
    let entries = header.partition_entry_sectors()?;
    let backup_lba = total_sectors
        .checked_sub(1)
        .ok_or_else(|| "GPT target size is too small".to_string())?;
    let last_usable_lba = backup_lba
        .checked_sub(entries + 1)
        .ok_or_else(|| "GPT target size is too small for partition entries".to_string())?;
    Ok((backup_lba, last_usable_lba))
}

pub(crate) fn modify_primary(data: &mut [u8], total_size_bytes: u64) -> Result<u64, String> {
    let mut header = GptHeaderRaw::parse(data)?;
    let (backup_lba, last_usable_lba) = target_lbas(&header, total_size_bytes)?;
    if last_usable_lba < header.last_usable_lba {
        header.last_usable_lba = last_usable_lba;
        header.alternate_lba = backup_lba;
        header.update_crc32();
        header.write_at_header_offset(data)?;
    }
    Ok(header.last_usable_lba)
}

pub(crate) fn modify_backup(
    backup_data: &mut [u8],
    total_size_bytes: u64,
    primary_data: &[u8],
) -> Result<(), String> {
    let primary = GptHeaderRaw::parse(primary_data)?;
    let (backup_lba, last_usable_lba) = target_lbas(&primary, total_size_bytes)?;
    let mut backup = primary;
    backup.my_lba = backup_lba;
    backup.alternate_lba = 1;
    backup.last_usable_lba = last_usable_lba;
    backup.partition_entry_lba = backup_lba
        .checked_sub(primary.partition_entry_sectors()?)
        .ok_or_else(|| "GPT target size is too small for backup entries".to_string())?;
    backup.update_crc32();
    backup.write_at_header_offset(backup_data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_short_and_invalid_gpt_data() {
        assert!(modify_primary(&mut [], 1024 * 1024).is_err());
        let mut invalid = vec![0; GPT_MIN_HEADER_SIZE];
        assert!(modify_primary(&mut invalid, 1024 * 1024).is_err());
    }
}
