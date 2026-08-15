//! OpenixCLI-cli library
//!
//! This crate provides firmware flashing functionality for Allwinner chips.
//! It can be used as a library or via the CLI tool.

pub mod cli;
pub mod commands;
pub mod config;
pub mod convert;
pub mod firmware;
pub mod flash;
pub mod process;
pub mod raw;
pub mod tui;
pub mod utils;

#[cfg(test)]
pub(crate) mod test_support;

pub use firmware::OpenixPacker;
