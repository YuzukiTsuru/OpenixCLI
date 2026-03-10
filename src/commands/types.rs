//! Command type definitions
//!
//! Defines types and structures used by CLI commands

use std::path::PathBuf;
use std::str::FromStr;

/// Arguments for the flash command
///
/// # Fields
/// * `firmware_path` - Path to the firmware file
/// * `bus` - USB bus number (optional)
/// * `port` - USB port number (optional)
/// * `verify` - Enable verification after write
/// * `mode` - Flash mode
/// * `partitions` - Specific partitions to flash (optional)
/// * `post_action` - Action to perform after flashing
/// * `verbose` - Enable verbose output
pub struct FlashArgs {
    pub firmware_path: PathBuf,
    pub bus: Option<u8>,
    pub port: Option<u8>,
    pub verify: bool,
    pub mode: FlashMode,
    pub partitions: Option<Vec<String>>,
    pub post_action: String,
    pub verbose: bool,
}

/// Flash mode options
///
/// # Variants
/// * `Partition` - Flash only specified partitions
/// * `KeepData` - Keep existing data
/// * `PartitionErase` - Erase partitions before flashing
/// * `FullErase` - Erase all data before flashing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlashMode {
    Partition,
    KeepData,
    PartitionErase,
    FullErase,
}

impl FromStr for FlashMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "partition" => Ok(Self::Partition),
            "keep_data" => Ok(Self::KeepData),
            "partition_erase" => Ok(Self::PartitionErase),
            "full_erase" => Ok(Self::FullErase),
            _ => Err(format!("Invalid flash mode: {}", s)),
        }
    }
}

impl std::fmt::Display for FlashMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FlashMode::Partition => write!(f, "partition"),
            FlashMode::KeepData => write!(f, "keep_data"),
            FlashMode::PartitionErase => write!(f, "partition_erase"),
            FlashMode::FullErase => write!(f, "full_erase"),
        }
    }
}
