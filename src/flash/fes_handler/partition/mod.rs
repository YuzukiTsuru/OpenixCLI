//! Partition download module
//!
//! Handles downloading partition data to device storage
//! Supports both raw and sparse partition formats

mod mod_impl;
mod raw_download;
mod sparse_parser;

pub use mod_impl::PartitionDownload;
