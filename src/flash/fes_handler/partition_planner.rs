//! Partition download planning.

use crate::config::mbr_parser::MbrInfo;
use crate::config::partition::{OpenixPartition, PartitionConfig};
use crate::firmware::OpenixPacker;
use crate::flash::{FlashMode, FlashRequest};
use crate::utils::{FlashResult, Logger};

use super::types::{PartitionDownloadInfo, ITEM_ROOTFSFAT16};

/// Builds the list of partition images that should be written for a request.
pub struct PartitionPlanner<'a> {
    logger: &'a Logger,
}

impl<'a> PartitionPlanner<'a> {
    pub fn new(logger: &'a Logger) -> Self {
        Self { logger }
    }

    pub fn prepare(
        &self,
        packer: &mut OpenixPacker,
        mbr_info: &MbrInfo,
        request: &FlashRequest,
    ) -> FlashResult<Vec<PartitionDownloadInfo>> {
        let mut partition_parser = OpenixPartition::new();

        if let Ok(data) = packer.get_sys_partition() {
            partition_parser.parse_from_data(&data);
        }

        let config_partitions = partition_parser.get_partitions();
        let mut download_list = Vec::new();

        for mbr_partition in &mbr_info.partitions {
            let partition_name = &mbr_partition.name;

            if !should_include_partition(
                request.mode,
                request.partitions.as_deref(),
                partition_name,
            ) {
                self.log_skip_reason(request.mode, partition_name);
                continue;
            }

            let Some(download_filename) =
                download_filename_for(config_partitions, partition_name).map(str::to_string)
            else {
                self.logger.debug(&format!(
                    "Partition {} has no download file, skipping",
                    partition_name
                ));
                continue;
            };

            let download_subtype = packer.build_subtype_by_filename(&download_filename);

            let data_info = packer
                .get_file_info_by_maintype_subtype(ITEM_ROOTFSFAT16, &download_subtype)
                .or_else(|| packer.get_file_info_by_maintype_subtype("12345678", &download_subtype))
                .or_else(|| packer.get_file_info_by_filename(&download_filename));

            if let Some((offset, length)) = data_info {
                download_list.push(PartitionDownloadInfo {
                    partition_name: partition_name.clone(),
                    partition_address: mbr_partition.address(),
                    download_filename,
                    download_subtype,
                    data_offset: offset,
                    data_length: length,
                });
            } else {
                self.logger.warn(&format!(
                    "Partition image not found: {} ({})",
                    partition_name, download_filename
                ));
            }
        }

        Ok(download_list)
    }

    fn log_skip_reason(&self, mode: FlashMode, partition_name: &str) {
        match mode {
            FlashMode::KeepData => self
                .logger
                .info(&format!("Skipping user data partition: {}", partition_name)),
            FlashMode::Partition => self.logger.info(&format!(
                "Skipping partition not in list: {}",
                partition_name
            )),
            _ => {}
        }
    }
}

pub fn should_include_partition(
    mode: FlashMode,
    selected_partitions: Option<&[String]>,
    partition_name: &str,
) -> bool {
    if mode == FlashMode::KeepData && is_user_data_partition(partition_name) {
        return false;
    }

    if mode == FlashMode::Partition {
        if let Some(partitions) = selected_partitions {
            return partitions.iter().any(|part| part == partition_name);
        }
    }

    true
}

fn is_user_data_partition(partition_name: &str) -> bool {
    matches!(
        partition_name.to_lowercase().as_str(),
        "udisk" | "private" | "reserve"
    )
}

fn download_filename_for<'a>(
    config_partitions: &'a [PartitionConfig],
    partition_name: &str,
) -> Option<&'a str> {
    config_partitions
        .iter()
        .find(|partition| partition.name == partition_name)
        .and_then(|partition| {
            if partition.downloadfile.is_empty() {
                None
            } else {
                Some(partition.downloadfile.as_str())
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn keep_data_skips_known_user_data_partitions() {
        for partition in ["udisk", "private", "reserve", "UDISK"] {
            assert!(!should_include_partition(
                FlashMode::KeepData,
                None,
                partition
            ));
        }

        assert!(should_include_partition(FlashMode::KeepData, None, "boot"));
    }

    #[test]
    fn partition_mode_filters_when_selection_is_present() {
        let selected = names(&["boot", "system"]);
        assert!(should_include_partition(
            FlashMode::Partition,
            Some(&selected),
            "boot"
        ));
        assert!(!should_include_partition(
            FlashMode::Partition,
            Some(&selected),
            "vendor"
        ));
    }

    #[test]
    fn partition_mode_without_selection_keeps_existing_all_partition_behavior() {
        assert!(should_include_partition(FlashMode::Partition, None, "boot"));
    }
}
