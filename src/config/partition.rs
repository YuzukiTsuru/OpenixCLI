//! Partition configuration parser
//!
//! Provides parsers for partition configuration files in INI-like format

#![allow(dead_code)]

/// Partition configuration entry
///
/// # Fields
/// * `name` - Partition name
/// * `size` - Partition size in bytes
/// * `downloadfile` - File to download to this partition
/// * `user_type` - User type identifier
/// * `keydata` - Contains key data
/// * `encrypt` - Should be encrypted
/// * `verify` - Should be verified after write
/// * `readonly` - Read-only partition
#[derive(Debug, Clone, Default)]
pub struct PartitionConfig {
    pub name: String,
    pub size: u64,
    pub downloadfile: String,
    pub user_type: u32,
    pub keydata: bool,
    pub encrypt: bool,
    pub verify: bool,
    pub readonly: bool,
}

/// Partition configuration container
pub struct OpenixPartition {
    partitions: Vec<PartitionConfig>,
}

impl OpenixPartition {
    /// Create a new empty partition configuration
    pub fn new() -> Self {
        Self {
            partitions: Vec::new(),
        }
    }

    /// Parse partition configuration from binary data
    pub fn parse_from_data(&mut self, data: &[u8]) -> bool {
        let content = String::from_utf8_lossy(data);
        self.parse_from_content(&content)
    }

    /// Parse partition configuration from string content
    fn parse_from_content(&mut self, content: &str) -> bool {
        self.partitions.clear();

        let mut in_partition_section = false;
        let mut current_partition = PartitionConfig::default();

        for line in content.lines() {
            let line = line.trim();

            if line.is_empty() || line.starts_with(';') || line.starts_with("//") {
                continue;
            }

            if line == "[partition_start]" {
                in_partition_section = true;
                continue;
            }

            if line == "[partition]" {
                if !current_partition.name.is_empty() {
                    self.partitions.push(current_partition.clone());
                }
                current_partition = PartitionConfig::default();
                in_partition_section = true;
                continue;
            }

            if line.starts_with('[') && line.ends_with(']') {
                if line != "[partition]" && line != "[partition_start]" && line != "[mbr]" {
                    if in_partition_section && !current_partition.name.is_empty() {
                        self.partitions.push(current_partition.clone());
                        current_partition = PartitionConfig::default();
                    }
                    in_partition_section = false;
                }
                continue;
            }

            if in_partition_section {
                self.parse_partition_line(line, &mut current_partition);
            }
        }

        if in_partition_section && !current_partition.name.is_empty() {
            self.partitions.push(current_partition);
        }

        true
    }

    /// Parse a single partition configuration line
    fn parse_partition_line(&self, line: &str, partition: &mut PartitionConfig) {
        let parts: Vec<&str> = line.splitn(2, '=').collect();
        if parts.len() != 2 {
            return;
        }

        let key = parts[0].trim();
        let value = parts[1].trim();
        let value = value.trim_matches('"');

        match key {
            "name" => partition.name = value.to_string(),
            "size" => {
                partition.size = if value.starts_with("0x") || value.starts_with("0X") {
                    u64::from_str_radix(&value[2..], 16).unwrap_or(0)
                } else {
                    value.parse().unwrap_or(0)
                }
            }
            "downloadfile" => partition.downloadfile = value.to_string(),
            "user_type" => {
                partition.user_type = if value.starts_with("0x") || value.starts_with("0X") {
                    u32::from_str_radix(&value[2..], 16).unwrap_or(0)
                } else {
                    value.parse().unwrap_or(0)
                }
            }
            "keydata" => partition.keydata = value != "0",
            "encrypt" => partition.encrypt = value != "0",
            "verify" => partition.verify = value != "0",
            "ro" => partition.readonly = value != "0",
            _ => {}
        }
    }

    /// Get all partitions
    pub fn get_partitions(&self) -> &[PartitionConfig] {
        &self.partitions
    }

    /// Get partition by name
    pub fn get_partition_by_name(&self, name: &str) -> Option<&PartitionConfig> {
        self.partitions.iter().find(|p| p.name == name)
    }
}

impl Default for OpenixPartition {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_multiple_partition_blocks() {
        let data = br#"
            ; comment
            [partition_start]
            [partition]
            name = boot
            size = 0x4000
            downloadfile = "boot.fex"
            user_type = 0x8000
            keydata = 1
            encrypt = 0
            verify = 1
            ro = 1

            [partition]
            name = system
            size = 8192
            downloadfile = system.img
        "#;

        let mut parser = OpenixPartition::new();
        assert!(parser.parse_from_data(data));

        let partitions = parser.get_partitions();
        assert_eq!(partitions.len(), 2);
        assert_eq!(partitions[0].name, "boot");
        assert_eq!(partitions[0].size, 0x4000);
        assert_eq!(partitions[0].downloadfile, "boot.fex");
        assert_eq!(partitions[0].user_type, 0x8000);
        assert!(partitions[0].keydata);
        assert!(!partitions[0].encrypt);
        assert!(partitions[0].verify);
        assert!(partitions[0].readonly);
        assert_eq!(partitions[1].name, "system");
        assert_eq!(partitions[1].size, 8192);
    }

    #[test]
    fn ignores_lines_outside_partition_sections() {
        let data = br#"
            [mbr]
            name = ignored
            [partition_start]
            [partition]
            name = vendor
            downloadfile = vendor.img
            [other]
            name = ignored-too
        "#;

        let mut parser = OpenixPartition::new();
        assert!(parser.parse_from_data(data));

        let partitions = parser.get_partitions();
        assert_eq!(partitions.len(), 1);
        assert_eq!(partitions[0].name, "vendor");
        assert_eq!(partitions[0].downloadfile, "vendor.img");
    }

    #[test]
    fn defaults_are_empty_and_lookup_handles_missing_names() {
        let parser = OpenixPartition::default();
        assert!(parser.get_partitions().is_empty());
        assert!(parser.get_partition_by_name("missing").is_none());
    }

    #[test]
    fn malformed_values_fall_back_without_panicking() {
        let data = br#"
            [partition_start]
            name = userdata
            size = 0xnot-hex
            user_type = no-number
            keydata = 2
            encrypt = -1
            verify = 0
            ro = 0
            ignored line
            unknown = value
        "#;
        let mut parser = OpenixPartition::new();
        assert!(parser.parse_from_data(data));
        let partition = parser.get_partition_by_name("userdata").unwrap();
        assert_eq!(partition.size, 0);
        assert_eq!(partition.user_type, 0);
        assert!(partition.keydata);
        assert!(partition.encrypt);
        assert!(!partition.verify);
        assert!(!partition.readonly);
    }

    #[test]
    fn parsing_again_replaces_previous_results_and_accepts_lossy_utf8() {
        let mut parser = OpenixPartition::new();
        parser.parse_from_data(b"[partition_start]\nname=first\n");
        assert!(parser.get_partition_by_name("first").is_some());

        parser.parse_from_data(b"[partition_start]\nname=second\xff\nsize=42\n");
        assert_eq!(parser.get_partitions().len(), 1);
        assert!(parser.get_partition_by_name("first").is_none());
        assert_eq!(parser.get_partitions()[0].size, 42);
    }
}
