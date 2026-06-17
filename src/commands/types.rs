//! Command type definitions
//!
//! Defines types and structures used by CLI commands

use std::path::PathBuf;

use crate::flash::{DeviceSelector, FlashMode, FlashRequest, PostAction};

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
    pub post_action: PostAction,
    pub verbose: bool,
}

impl FlashArgs {
    pub fn request(&self) -> FlashRequest {
        FlashRequest::new(
            DeviceSelector::new(self.bus, self.port),
            self.verify,
            self.mode,
            self.partitions.clone(),
            self.post_action,
        )
    }
}

/// Arguments for the unpack command
///
/// # Fields
/// * `firmware_path` - Path to the firmware file
/// * `output` - Optional output directory (defaults to ./<firmware>_unpacked)
pub struct UnpackArgs {
    pub firmware_path: PathBuf,
    pub output: Option<PathBuf>,
}

pub fn parse_partition_list(partitions: Option<String>) -> Option<Vec<String>> {
    partitions.map(|value| {
        value
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(str::to_string)
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_partition_list_trims_and_drops_empty_values() {
        assert_eq!(
            parse_partition_list(Some(" boot, system ,,vendor ".to_string())),
            Some(vec![
                "boot".to_string(),
                "system".to_string(),
                "vendor".to_string()
            ])
        );
    }

    #[test]
    fn flash_args_builds_shared_request() {
        let args = FlashArgs {
            firmware_path: PathBuf::from("firmware.img"),
            bus: Some(1),
            port: Some(2),
            verify: false,
            mode: FlashMode::Partition,
            partitions: Some(vec!["boot".to_string()]),
            post_action: PostAction::PowerOff,
            verbose: true,
        };

        let request = args.request();
        assert_eq!(request.device.selected_pair(), Some((1, 2)));
        assert_eq!(request.mode, FlashMode::Partition);
        assert_eq!(request.post_action, PostAction::PowerOff);
        assert!(!request.verify);
        assert_eq!(request.partitions, Some(vec!["boot".to_string()]));
    }
}
