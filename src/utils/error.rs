//! Error handling module
//!
//! Provides error types and result types for flash operations

#![allow(dead_code)]

use thiserror::Error;

/// Flash operation errors
///
/// # Variants
/// * `FirmwareNotFound` - Firmware file not found
/// * `InvalidFirmwareFormat` - Invalid firmware format
/// * `EncryptedNotSupported` - Encrypted firmware not supported
/// * `DeviceNotFound` - No device found
/// * `DeviceOpenFailed` - Failed to open device
/// * `DramInitFailed` - DRAM initialization failed
/// * `UbootDownloadFailed` - U-Boot download failed
/// * `MbrDownloadFailed` - MBR download failed
/// * `PartitionDownloadFailed` - Partition download failed
/// * `ReconnectFailed` - Device reconnect failed
/// * `StorageTypeMismatch` - Storage type mismatch
/// * `FesNotFound` - FES not found in firmware
/// * `UbootNotFound` - U-Boot not found in firmware
/// * `SysConfigNotFound` - SysConfig not found in firmware
/// * `MbrNotFound` - MBR not found in firmware
/// * `Boot0NotFound` - Boot0 not found in firmware
/// * `Boot1NotFound` - Boot1 not found in firmware
/// * `UsbTransferError` - USB transfer error
/// * `Cancelled` - Operation cancelled
/// * `Timeout` - Operation timeout
/// * `Io` - IO error
/// * `Packer` - Packer error
/// * `Libefex` - Libefex error
/// * `Unknown` - Unknown error
#[derive(Debug, Error)]
pub enum FlashError {
    #[error("Firmware file not found: {0}")]
    FirmwareNotFound(String),

    #[error("Invalid firmware format: {0}")]
    InvalidFirmwareFormat(String),

    #[error("Encrypted firmware not supported")]
    EncryptedNotSupported,

    #[error("Device not found")]
    DeviceNotFound,

    #[error("Failed to open device: {0}")]
    DeviceOpenFailed(String),

    #[error("DRAM initialization failed")]
    DramInitFailed,

    #[error("U-Boot download failed")]
    UbootDownloadFailed,

    #[error("MBR download failed")]
    MbrDownloadFailed,

    #[error("Partition download failed: {0}")]
    PartitionDownloadFailed(String),

    #[error("Device reconnect failed")]
    ReconnectFailed,

    #[error("Storage type mismatch: device={device}, firmware={firmware}")]
    StorageTypeMismatch { device: String, firmware: String },

    #[error("FES not found in firmware")]
    FesNotFound,

    #[error("U-Boot not found in firmware")]
    UbootNotFound,

    #[error("SysConfig not found in firmware")]
    SysConfigNotFound,

    #[error("MBR not found in firmware")]
    MbrNotFound,

    #[error("Boot0 not found in firmware")]
    Boot0NotFound,

    #[error("Boot1 not found in firmware")]
    Boot1NotFound,

    #[error("USB transfer error: {0}")]
    UsbTransferError(String),

    #[error("Operation cancelled")]
    Cancelled,

    #[error("Timeout: {0}")]
    Timeout(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Packer error: {0}")]
    Packer(#[from] crate::firmware::PackerError),

    #[error("Libefex error: {0}")]
    Libefex(#[from] libefex::EfexError),

    #[error("Unknown error: {0}")]
    Unknown(String),
}

/// Result type for flash operations
pub type FlashResult<T> = Result<T, FlashError>;
