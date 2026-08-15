//! Detection of conversion offsets from an embedded Allwinner flash map DTB.

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct FlashOffsets {
    pub(crate) sdmmc_logic_offset: Option<u64>,
    pub(crate) nor_logic_offset: Option<u64>,
    pub(crate) nor_uboot_start: Option<u64>,
}

fn property_u32(node: &fdt::node::FdtNode<'_, '_>, name: &str) -> Option<u64> {
    let value = node.property(name)?.value;
    let bytes: [u8; 4] = value.get(..4)?.try_into().ok()?;
    Some(u64::from(u32::from_be_bytes(bytes)))
}

pub(crate) fn parse_flash_offsets(data: &[u8]) -> Result<FlashOffsets, String> {
    let tree = fdt::Fdt::new(data).map_err(|error| format!("Failed to parse DTB: {error:?}"))?;
    let flashmap = tree
        .find_node("/soc/sunxi_flashmap")
        .ok_or_else(|| "sunxi_flashmap node not found".to_string())?;

    let mut offsets = FlashOffsets::default();
    for child in flashmap.children() {
        match child.name {
            "sdmmc_map" => {
                offsets.sdmmc_logic_offset = property_u32(&child, "logic_offset");
            }
            "nor_map" => {
                offsets.nor_logic_offset = property_u32(&child, "logic_offset");
                offsets.nor_uboot_start = property_u32(&child, "uboot_start");
            }
            _ => {}
        }
    }
    Ok(offsets)
}

pub(crate) fn extract_dtb_from_uboot(data: &[u8]) -> Option<&[u8]> {
    const DTB_MAGIC: [u8; 4] = [0xd0, 0x0d, 0xfe, 0xed];

    let mut offset = data.len() & !3;
    while offset >= 4 {
        offset -= 4;
        if data[offset..offset + 4] != DTB_MAGIC || offset + 8 > data.len() {
            continue;
        }
        let total_size = u32::from_be_bytes(data[offset + 4..offset + 8].try_into().ok()?) as usize;
        let end = offset.checked_add(total_size)?;
        if total_size > 0 && end <= data.len() {
            let candidate = &data[offset..end];
            if fdt::Fdt::new(candidate).is_ok() {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_data_has_no_flash_map_or_appended_dtb() {
        assert!(parse_flash_offsets(&[]).is_err());
        assert_eq!(extract_dtb_from_uboot(&[]), None);
        assert_eq!(extract_dtb_from_uboot(&[0xd0, 0x0d, 0xfe, 0xed]), None);
    }
}
