//! Firmware parsing and packing modules
//!
//! Provides functionality for parsing and extracting Allwinner firmware files (.fex)
//! Supports IMAGEWTY format firmware images

pub mod image_data;
pub mod packer;
pub mod sparse;
pub mod types;

pub use packer::OpenixPacker;
pub use packer::PackerError;
pub use types::*;
