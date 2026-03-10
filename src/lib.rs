//! OpenixSuit-cli library
//!
//! This crate provides firmware flashing functionality for Allwinner chips.
//! It can be used as a library or via the CLI tool.

pub mod commands;
pub mod config;
pub mod firmware;
pub mod flash;
pub mod process;
pub mod utils;

pub use firmware::OpenixPacker;
