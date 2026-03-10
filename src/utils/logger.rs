//! Logger implementation
//!
//! Provides logging and progress reporting functionality for flash operations

use super::terminal::{log_debug, log_error, log_info, log_stage_complete, log_success, log_warn};
use crate::process::{ProgressReporter, StageType};
use std::sync::Arc;

/// Logger
///
/// Provides a unified interface for logging and progress reporting
#[derive(Clone)]
pub struct Logger {
    verbose: bool,
    reporter: Arc<ProgressReporter>,
}

impl Logger {
    /// Create a new logger with default settings
    pub fn new() -> Self {
        Self {
            verbose: false,
            reporter: Arc::new(ProgressReporter::new()),
        }
    }

    /// Create a new logger with verbose mode
    pub fn with_verbose(verbose: bool) -> Self {
        Self {
            verbose,
            reporter: Arc::new(ProgressReporter::new()),
        }
    }

    /// Log an info message
    pub fn info(&self, message: &str) {
        log_info(message);
    }

    /// Log a success message
    #[allow(dead_code)]
    pub fn success(&self, message: &str) {
        log_success(message);
    }

    /// Log a warning message
    pub fn warn(&self, message: &str) {
        log_warn(message);
    }

    /// Log an error message
    pub fn error(&self, message: &str) {
        log_error(message);
    }

    /// Log a debug message (only if verbose mode is enabled)
    pub fn debug(&self, message: &str) {
        if self.verbose {
            log_debug(message);
        }
    }

    /// Log a stage completion message
    pub fn stage_complete(&self, message: &str) {
        log_stage_complete(message);
    }

    /// Start global progress tracking
    pub fn start_global_progress(&self) {
        self.reporter.start();
    }

    /// Define stages for progress tracking
    pub fn define_stages(&self, stages: &[StageType]) {
        self.reporter.define_stages(stages);
    }

    /// Begin a specific stage
    pub fn begin_stage(&self, stage_type: StageType) {
        self.reporter.begin_stage(stage_type);
    }

    /// Set partition stage weight for progress calculation
    pub fn set_partition_stage_weight(&self, total_bytes: u64) {
        self.reporter.set_partition_stage_weight(total_bytes);
    }

    /// Set current partition name for display
    pub fn set_current_partition(&self, partition_name: &str) {
        self.reporter.set_current_partition(partition_name);
    }

    /// Update progress (bytes written)
    #[allow(dead_code)]
    pub fn update_progress(&self, current: u64) {
        self.reporter.update_progress(current);
    }

    /// Update progress with speed calculation
    pub fn update_progress_with_speed(&self, current: u64) {
        self.reporter.update_progress_with_speed(current);
    }

    /// Mark current stage as completed
    pub fn complete_stage(&self) {
        self.reporter.complete_stage();
    }

    /// Finish progress tracking
    pub fn finish_progress(&self) {
        self.reporter.finish();
    }

    /// Update progress by percentage
    #[allow(dead_code)]
    pub fn update_progress_percent(&self, percent: u8) {
        self.reporter.update_progress_percent(percent);
    }

    /// Get current progress percentage (0-100)
    #[allow(dead_code)]
    pub fn get_progress(&self) -> u8 {
        self.reporter.get_progress()
    }
}

impl Default for Logger {
    fn default() -> Self {
        Self::new()
    }
}
