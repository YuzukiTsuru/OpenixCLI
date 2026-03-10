//! Progress reporter
//!
//! Provides a simplified interface for reporting flash operation progress

use super::global_progress::{global_progress, StageType};
use std::sync::Arc;

/// Progress reporter
///
/// A wrapper around GlobalProgress that provides a simpler interface
/// for reporting flash operation progress
pub struct ProgressReporter {
    progress: Arc<super::global_progress::GlobalProgress>,
}

impl ProgressReporter {
    /// Create a new progress reporter
    pub fn new() -> Self {
        Self {
            progress: global_progress(),
        }
    }

    /// Start progress tracking
    pub fn start(&self) {
        self.progress.start();
    }

    /// Define the stages for the flash operation
    pub fn define_stages(&self, stages: &[StageType]) {
        self.progress.define_stages(stages);
    }

    /// Begin a specific stage
    pub fn begin_stage(&self, stage_type: StageType) {
        self.progress.start_stage(stage_type);
    }

    /// Set the weight for partition flashing stage
    pub fn set_partition_stage_weight(&self, total_bytes: u64) {
        self.progress.set_partition_stage_weight(total_bytes);
    }

    /// Set the current partition name
    pub fn set_current_partition(&self, partition_name: &str) {
        self.progress.set_current_partition(partition_name);
    }

    /// Update progress (bytes written)
    pub fn update_progress(&self, current: u64) {
        self.progress.update_stage_progress(current);
    }

    /// Update progress with speed calculation
    pub fn update_progress_with_speed(&self, current: u64) {
        self.progress.update_stage_progress_with_speed(current);
    }

    /// Update progress by percentage
    pub fn update_progress_percent(&self, percent: u8) {
        self.progress.update_stage_progress(percent as u64);
    }

    /// Mark current stage as completed
    pub fn complete_stage(&self) {
        self.progress.complete_stage();
    }

    /// Finish progress tracking
    pub fn finish(&self) {
        self.progress.finish();
    }

    /// Get current progress percentage (0-100)
    pub fn get_progress(&self) -> u8 {
        self.progress.get_progress()
    }
}

impl Default for ProgressReporter {
    fn default() -> Self {
        Self::new()
    }
}
