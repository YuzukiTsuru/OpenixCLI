//! Utility modules
//!
//! Provides common utilities including:
//! - Error handling
//! - Logging
//! - Terminal utilities

pub mod error;
pub mod logger;
pub mod terminal;

pub use error::{FlashError, FlashResult};
pub use logger::Logger;
pub use terminal::TermLogger;
