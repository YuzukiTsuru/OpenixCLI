use std::path::PathBuf;
use std::str::FromStr;

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
