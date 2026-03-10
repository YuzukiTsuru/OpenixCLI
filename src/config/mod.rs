//! Configuration parsing modules
//!
//! Provides parsers for various configuration formats used in Allwinner firmware:
//! - Boot headers (boot0, U-Boot)
//! - MBR partition tables
//! - Partition configurations
//! - System configurations

pub mod boot_header;
pub mod mbr_parser;
pub mod partition;
pub mod sys_config;
