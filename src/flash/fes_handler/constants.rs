//! FES handler constants
//!
//! Provides constants for FES operations

/// Maximum chunk size for data transfers (256 MB)
pub const CHUNK_SIZE: u64 = 256 * 1024 * 1024;
/// Buffer size for data operations (256 KB)
pub const BUFFER_SIZE: usize = 256 * 1024;
/// Interval for speed calculation updates (64 KB)
pub const SPEED_UPDATE_INTERVAL: u64 = 64 * 1024;
