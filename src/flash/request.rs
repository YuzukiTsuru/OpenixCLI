//! Shared flash request and option types.

use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

/// Flash mode options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlashMode {
    /// Flash only specified partitions.
    Partition,
    /// Flash while preserving common user data partitions.
    KeepData,
    /// Erase selected partitions before flashing.
    PartitionErase,
    /// Full erase before flashing.
    FullErase,
}

impl FlashMode {
    /// Get erase flag for this mode.
    pub fn erase_flag(self) -> u32 {
        match self {
            FlashMode::Partition => 0x0,
            FlashMode::KeepData => 0x0,
            FlashMode::PartitionErase => 0x1,
            FlashMode::FullErase => 0x12,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            FlashMode::Partition => "Partition",
            FlashMode::KeepData => "Keep Data",
            FlashMode::PartitionErase => "Part. Erase",
            FlashMode::FullErase => "Full Erase",
        }
    }

    pub fn next(self) -> Self {
        match self {
            FlashMode::FullErase => FlashMode::PartitionErase,
            FlashMode::PartitionErase => FlashMode::KeepData,
            FlashMode::KeepData => FlashMode::Partition,
            FlashMode::Partition => FlashMode::FullErase,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            FlashMode::FullErase => FlashMode::Partition,
            FlashMode::Partition => FlashMode::KeepData,
            FlashMode::KeepData => FlashMode::PartitionErase,
            FlashMode::PartitionErase => FlashMode::FullErase,
        }
    }
}

impl FromStr for FlashMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "partition" => Ok(Self::Partition),
            "keep_data" => Ok(Self::KeepData),
            "partition_erase" => Ok(Self::PartitionErase),
            "full_erase" => Ok(Self::FullErase),
            _ => Err(format!("Invalid flash mode: {}", value)),
        }
    }
}

impl fmt::Display for FlashMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FlashMode::Partition => write!(f, "partition"),
            FlashMode::KeepData => write!(f, "keep_data"),
            FlashMode::PartitionErase => write!(f, "partition_erase"),
            FlashMode::FullErase => write!(f, "full_erase"),
        }
    }
}

/// Action to perform after flashing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostAction {
    Reboot,
    PowerOff,
    Shutdown,
}

impl PostAction {
    pub fn name(self) -> &'static str {
        match self {
            PostAction::Reboot => "Reboot",
            PostAction::PowerOff => "Power Off",
            PostAction::Shutdown => "Shutdown",
        }
    }

    pub fn next(self) -> Self {
        match self {
            PostAction::Reboot => PostAction::PowerOff,
            PostAction::PowerOff => PostAction::Shutdown,
            PostAction::Shutdown => PostAction::Reboot,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            PostAction::Reboot => PostAction::Shutdown,
            PostAction::PowerOff => PostAction::Reboot,
            PostAction::Shutdown => PostAction::PowerOff,
        }
    }

    pub fn fes_tool_mode(self) -> libefex::FesToolMode {
        match self {
            PostAction::Reboot => libefex::FesToolMode::Reboot,
            PostAction::PowerOff | PostAction::Shutdown => libefex::FesToolMode::PowerOff,
        }
    }
}

impl FromStr for PostAction {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "reboot" => Ok(Self::Reboot),
            "poweroff" => Ok(Self::PowerOff),
            "shutdown" => Ok(Self::Shutdown),
            _ => Err(format!("Invalid post-flash action: {}", value)),
        }
    }
}

impl fmt::Display for PostAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PostAction::Reboot => write!(f, "reboot"),
            PostAction::PowerOff => write!(f, "poweroff"),
            PostAction::Shutdown => write!(f, "shutdown"),
        }
    }
}

/// Device selection requested by the caller.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DeviceSelector {
    pub bus: Option<u8>,
    pub port: Option<u8>,
}

impl DeviceSelector {
    pub fn new(bus: Option<u8>, port: Option<u8>) -> Self {
        Self { bus, port }
    }

    pub fn selected_pair(self) -> Option<(u8, u8)> {
        match (self.bus, self.port) {
            (Some(bus), Some(port)) => Some((bus, port)),
            _ => None,
        }
    }
}

/// A partition whose payload is read from a standalone file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalPartition {
    pub name: String,
    pub path: PathBuf,
    pub address: u64,
    pub data_length: u64,
    pub wrap_address: bool,
}

impl ExternalPartition {
    pub fn new(
        name: impl Into<String>,
        path: impl AsRef<Path>,
        address: u64,
        data_length: u64,
    ) -> Self {
        Self {
            name: name.into(),
            path: path.as_ref().to_path_buf(),
            address,
            data_length,
            wrap_address: false,
        }
    }

    pub fn with_wrapping_address(mut self, wrap_address: bool) -> Self {
        self.wrap_address = wrap_address;
        self
    }
}

/// An MBR and external partition list supplied by a specialized flash command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomFlashLayout {
    pub mbr_data: Vec<u8>,
    pub partitions: Vec<ExternalPartition>,
}

impl CustomFlashLayout {
    pub fn new(mbr_data: Vec<u8>, partitions: Vec<ExternalPartition>) -> Self {
        Self {
            mbr_data,
            partitions,
        }
    }
}

/// Fully resolved flash request used by CLI and TUI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlashRequest {
    pub device: DeviceSelector,
    pub verify: bool,
    pub mode: FlashMode,
    pub partitions: Option<Vec<String>>,
    pub post_action: PostAction,
    pub custom_layout: Option<CustomFlashLayout>,
}

impl FlashRequest {
    pub fn new(
        device: DeviceSelector,
        verify: bool,
        mode: FlashMode,
        partitions: Option<Vec<String>>,
        post_action: PostAction,
    ) -> Self {
        Self {
            device,
            verify,
            mode,
            partitions,
            post_action,
            custom_layout: None,
        }
    }

    pub fn with_custom_layout(mut self, layout: CustomFlashLayout) -> Self {
        self.custom_layout = Some(layout);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flash_mode_round_trips_cli_values() {
        let cases = [
            ("partition", FlashMode::Partition),
            ("keep_data", FlashMode::KeepData),
            ("partition_erase", FlashMode::PartitionErase),
            ("full_erase", FlashMode::FullErase),
        ];

        for (value, mode) in cases {
            assert_eq!(value.parse::<FlashMode>().unwrap(), mode);
            assert_eq!(mode.to_string(), value);
        }
    }

    #[test]
    fn post_action_round_trips_cli_values() {
        let cases = [
            ("reboot", PostAction::Reboot),
            ("poweroff", PostAction::PowerOff),
            ("shutdown", PostAction::Shutdown),
        ];

        for (value, action) in cases {
            assert_eq!(value.parse::<PostAction>().unwrap(), action);
            assert_eq!(action.to_string(), value);
        }
    }

    #[test]
    fn device_selector_requires_bus_and_port_for_explicit_selection() {
        assert_eq!(
            DeviceSelector::new(Some(1), Some(5)).selected_pair(),
            Some((1, 5))
        );
        assert_eq!(DeviceSelector::new(Some(1), None).selected_pair(), None);
        assert_eq!(DeviceSelector::new(None, Some(5)).selected_pair(), None);
        assert_eq!(DeviceSelector::default().selected_pair(), None);
    }

    #[test]
    fn flash_mode_properties_and_navigation_cover_full_cycle() {
        let cases = [
            (FlashMode::Partition, 0, "Partition"),
            (FlashMode::KeepData, 0, "Keep Data"),
            (FlashMode::PartitionErase, 1, "Part. Erase"),
            (FlashMode::FullErase, 0x12, "Full Erase"),
        ];
        for (mode, flag, name) in cases {
            assert_eq!(mode.erase_flag(), flag);
            assert_eq!(mode.display_name(), name);
            assert_eq!(mode.next().prev(), mode);
            assert_eq!(mode.prev().next(), mode);
        }
        assert!("FULL_ERASE".parse::<FlashMode>().is_err());
        assert_eq!(FlashMode::FullErase.next(), FlashMode::PartitionErase);
        assert_eq!(FlashMode::Partition.prev(), FlashMode::KeepData);
    }

    #[test]
    fn post_action_properties_navigation_and_protocol_mapping_cover_cycle() {
        let cases = [
            (PostAction::Reboot, "Reboot", libefex::FesToolMode::Reboot),
            (
                PostAction::PowerOff,
                "Power Off",
                libefex::FesToolMode::PowerOff,
            ),
            (
                PostAction::Shutdown,
                "Shutdown",
                libefex::FesToolMode::PowerOff,
            ),
        ];
        for (action, name, tool_mode) in cases {
            assert_eq!(action.name(), name);
            assert_eq!(action.fes_tool_mode(), tool_mode);
            assert_eq!(action.next().prev(), action);
            assert_eq!(action.prev().next(), action);
        }
        assert!("restart".parse::<PostAction>().is_err());
    }

    #[test]
    fn flash_request_constructor_preserves_all_options() {
        let request = FlashRequest::new(
            DeviceSelector::new(Some(2), Some(3)),
            false,
            FlashMode::KeepData,
            Some(vec!["boot".to_string()]),
            PostAction::Shutdown,
        );
        assert_eq!(request.device.selected_pair(), Some((2, 3)));
        assert!(!request.verify);
        assert_eq!(request.mode, FlashMode::KeepData);
        assert_eq!(request.partitions, Some(vec!["boot".to_string()]));
        assert_eq!(request.post_action, PostAction::Shutdown);
    }

    #[test]
    fn flash_request_can_carry_an_external_layout() {
        let layout = CustomFlashLayout::new(
            vec![1, 2, 3],
            vec![ExternalPartition::new("raw", "disk.img", 0x1234, 513)],
        );
        let request = FlashRequest::new(
            DeviceSelector::default(),
            false,
            FlashMode::FullErase,
            None,
            PostAction::Reboot,
        )
        .with_custom_layout(layout.clone());

        assert_eq!(request.custom_layout, Some(layout));
    }
}
