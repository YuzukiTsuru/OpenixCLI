use super::terminal::{log_debug, log_error, log_info, log_stage_complete, log_success, log_warn};
use crate::process::{ProgressReporter, StageType};
use std::sync::Arc;

#[derive(Clone)]
pub struct Logger {
    verbose: bool,
    reporter: Arc<ProgressReporter>,
}

impl Logger {
    pub fn new() -> Self {
        Self {
            verbose: false,
            reporter: Arc::new(ProgressReporter::new()),
        }
    }

    pub fn with_verbose(verbose: bool) -> Self {
        Self {
            verbose,
            reporter: Arc::new(ProgressReporter::new()),
        }
    }

    pub fn info(&self, message: &str) {
        log_info(message);
    }

    #[allow(dead_code)]
    pub fn success(&self, message: &str) {
        log_success(message);
    }

    pub fn warn(&self, message: &str) {
        log_warn(message);
    }

    pub fn error(&self, message: &str) {
        log_error(message);
    }

    pub fn debug(&self, message: &str) {
        if self.verbose {
            log_debug(message);
        }
    }

    pub fn stage_complete(&self, message: &str) {
        log_stage_complete(message);
    }

    pub fn start_global_progress(&self) {
        self.reporter.start();
    }

    pub fn define_stages(&self, stages: &[StageType]) {
        self.reporter.define_stages(stages);
    }

    pub fn begin_stage(&self, stage_type: StageType) {
        self.reporter.begin_stage(stage_type);
    }

    pub fn set_partition_stage_weight(&self, total_bytes: u64) {
        self.reporter.set_partition_stage_weight(total_bytes);
    }

    pub fn set_current_partition(&self, partition_name: &str) {
        self.reporter.set_current_partition(partition_name);
    }

    #[allow(dead_code)]
    pub fn update_progress(&self, current: u64) {
        self.reporter.update_progress(current);
    }

    pub fn update_progress_with_speed(&self, current: u64) {
        self.reporter.update_progress_with_speed(current);
    }

    pub fn complete_stage(&self) {
        self.reporter.complete_stage();
    }

    pub fn finish_progress(&self) {
        self.reporter.finish();
    }

    pub fn progress_update(&self, current: usize, _message: &str) {
        self.reporter.update_progress(current as u64);
    }

    #[allow(dead_code)]
    pub fn update_progress_percent(&self, percent: u8) {
        self.reporter.update_progress_percent(percent);
    }

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
