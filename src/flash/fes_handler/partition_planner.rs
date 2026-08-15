//! Partition download planning.

use crate::config::mbr_parser::MbrInfo;
use crate::config::partition::{OpenixPartition, PartitionConfig};
use crate::firmware::OpenixPacker;
use crate::flash::{FlashMode, FlashRequest};
use crate::utils::{FlashResult, Logger};

use super::types::{PartitionDownloadInfo, PartitionSource, ITEM_ROOTFSFAT16};

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
                    source: PartitionSource::Firmware,
                    wrap_address: false,
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
    use crate::config::mbr_parser::SunxiMbr;
    use crate::flash::{DeviceSelector, PostAction};
    use crate::test_support::{mbr_bytes, test_firmware, FirmwareEntry};

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
        assert!(should_include_partition(
            FlashMode::PartitionErase,
            Some(&names(&[])),
            "boot"
        ));
        assert!(should_include_partition(
            FlashMode::FullErase,
            Some(&names(&[])),
            "boot"
        ));

        let logger = Logger::for_events(false, crate::flash::FlashEventSink::none());
        let planner = PartitionPlanner::new(&logger);
        planner.log_skip_reason(FlashMode::KeepData, "udisk");
        planner.log_skip_reason(FlashMode::Partition, "vendor");
        planner.log_skip_reason(FlashMode::FullErase, "boot");
    }

    fn request(mode: FlashMode, partitions: Option<Vec<String>>) -> FlashRequest {
        FlashRequest::new(
            DeviceSelector::default(),
            true,
            mode,
            partitions,
            PostAction::Reboot,
        )
    }

    #[test]
    fn prepare_joins_mbr_config_and_primary_partition_image() {
        let config = b"[partition_start]\n[partition]\nname=system\ndownloadfile=system.img\n";
        let firmware = test_firmware(&[
            FirmwareEntry {
                filename: "sys_partition.fex",
                maintype: "COMMON",
                subtype: "SYS_CONFIG000000",
                data: config,
            },
            FirmwareEntry {
                filename: "system.img",
                maintype: ITEM_ROOTFSFAT16,
                subtype: "SYSTEM_IMG000000",
                data: b"payload",
            },
        ]);
        let mut packer = OpenixPacker::new();
        packer.load(firmware.path()).unwrap();
        let mbr = SunxiMbr::parse(&mbr_bytes(&[("system", 0x1234, 0x1000, false)]))
            .unwrap()
            .to_mbr_info();
        let logger = Logger::for_events(false, crate::flash::FlashEventSink::none());
        let list = PartitionPlanner::new(&logger)
            .prepare(&mut packer, &mbr, &request(FlashMode::FullErase, None))
            .unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].partition_name, "system");
        assert_eq!(list[0].partition_address, 0x1234);
        assert_eq!(list[0].download_filename, "system.img");
        assert_eq!(list[0].download_subtype, "SYSTEM_IMG000000");
        assert_eq!(list[0].data_length, 7);
    }

    #[test]
    fn prepare_uses_fallback_maintype_then_filename_lookup() {
        let config = b"[partition_start]\n[partition]\nname=boot\ndownloadfile=boot.fex\n[partition]\nname=vendor\ndownloadfile=vendor.img\n";
        let firmware = test_firmware(&[
            FirmwareEntry {
                filename: "sys_partition.fex",
                maintype: "COMMON",
                subtype: "SYS_CONFIG000000",
                data: config,
            },
            FirmwareEntry {
                filename: "boot.fex",
                maintype: "12345678",
                subtype: "BOOT_FEX00000000",
                data: b"boot",
            },
            FirmwareEntry {
                filename: "vendor.img",
                maintype: "OTHER",
                subtype: "UNRELATED0000000",
                data: b"vendor",
            },
        ]);
        let mut packer = OpenixPacker::new();
        packer.load(firmware.path()).unwrap();
        let mbr_data = mbr_bytes(&[("boot", 1, 1, false), ("vendor", 2, 1, false)]);
        let mbr = SunxiMbr::parse(&mbr_data).unwrap().to_mbr_info();
        let logger = Logger::for_events(false, crate::flash::FlashEventSink::none());
        let list = PartitionPlanner::new(&logger)
            .prepare(&mut packer, &mbr, &request(FlashMode::FullErase, None))
            .unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].data_length, 4);
        assert_eq!(list[1].data_length, 6);
    }

    #[test]
    fn prepare_skips_unselected_missing_config_empty_filename_and_missing_images() {
        let config = b"[partition_start]\n[partition]\nname=empty\ndownloadfile=\n[partition]\nname=missing\ndownloadfile=missing.img\n[partition]\nname=boot\ndownloadfile=boot.img\n";
        let firmware = test_firmware(&[FirmwareEntry {
            filename: "sys_partition.fex",
            maintype: "COMMON",
            subtype: "SYS_CONFIG000000",
            data: config,
        }]);
        let mut packer = OpenixPacker::new();
        packer.load(firmware.path()).unwrap();
        let mbr_data = mbr_bytes(&[
            ("no-config", 0, 0, false),
            ("empty", 0, 0, false),
            ("missing", 0, 0, false),
            ("boot", 0, 0, false),
        ]);
        let mbr = SunxiMbr::parse(&mbr_data).unwrap().to_mbr_info();
        let logger = Logger::for_events(false, crate::flash::FlashEventSink::none());
        let selected = request(FlashMode::Partition, Some(names(&["empty", "missing"])));
        let list = PartitionPlanner::new(&logger)
            .prepare(&mut packer, &mbr, &selected)
            .unwrap();
        assert!(list.is_empty());

        let no_config = test_firmware(&[]);
        let mut no_config_packer = OpenixPacker::new();
        no_config_packer.load(no_config.path()).unwrap();
        assert!(PartitionPlanner::new(&logger)
            .prepare(
                &mut no_config_packer,
                &mbr,
                &request(FlashMode::KeepData, None)
            )
            .unwrap()
            .is_empty());
    }
}
