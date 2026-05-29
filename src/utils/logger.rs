//! Logger implementation
//!
//! Provides logging and progress reporting functionality for flash operations

use super::terminal::{log_debug, log_error, log_info, log_stage_complete, log_success, log_warn};
use crate::flash::{FlashEvent, FlashEventSink, FlashLogLevel, PostAction};
use crate::process::{ProgressReporter, StageType};
use std::sync::Arc;

/// Logger
///
/// Provides a unified interface for logging and progress reporting
#[derive(Clone)]
pub struct Logger {
    verbose: bool,
    reporter: Arc<ProgressReporter>,
    event_sink: FlashEventSink,
    terminal_output: bool,
}

impl Logger {
    /// Create a new logger with default settings
    pub fn new() -> Self {
        Self {
            verbose: false,
            reporter: Arc::new(ProgressReporter::new()),
            event_sink: FlashEventSink::none(),
            terminal_output: true,
        }
    }

    /// Create a new logger with verbose mode
    pub fn with_verbose(verbose: bool) -> Self {
        Self {
            verbose,
            reporter: Arc::new(ProgressReporter::new()),
            event_sink: FlashEventSink::none(),
            terminal_output: true,
        }
    }

    /// Create a logger that emits events without writing terminal output.
    pub fn for_events(verbose: bool, event_sink: FlashEventSink) -> Self {
        Self {
            verbose,
            reporter: Arc::new(ProgressReporter::new()),
            event_sink,
            terminal_output: false,
        }
    }

    fn emit_log(&self, level: FlashLogLevel, message: &str) {
        self.event_sink.emit(FlashEvent::Log {
            level,
            message: message.to_string(),
        });
    }

    /// Log an info message
    pub fn info(&self, message: &str) {
        self.emit_log(FlashLogLevel::Info, message);
        if self.terminal_output {
            log_info(message);
        }
    }

    /// Log a success message
    #[allow(dead_code)]
    pub fn success(&self, message: &str) {
        self.emit_log(FlashLogLevel::Success, message);
        if self.terminal_output {
            log_success(message);
        }
    }

    /// Log a warning message
    pub fn warn(&self, message: &str) {
        self.emit_log(FlashLogLevel::Warn, message);
        if self.terminal_output {
            log_warn(message);
        }
    }

    /// Log an error message
    pub fn error(&self, message: &str) {
        self.emit_log(FlashLogLevel::Error, message);
        if self.terminal_output {
            log_error(message);
        }
    }

    /// Log a debug message (only if verbose mode is enabled)
    pub fn debug(&self, message: &str) {
        if self.verbose {
            self.emit_log(FlashLogLevel::Debug, message);
            if self.terminal_output {
                log_debug(message);
            }
        }
    }

    /// Log a stage completion message
    pub fn stage_complete(&self, message: &str) {
        self.emit_log(FlashLogLevel::Success, message);
        if self.terminal_output {
            log_stage_complete(message);
        }
    }

    /// Start global progress tracking
    pub fn start_global_progress(&self) {
        self.reporter.start();
    }

    /// Define stages for progress tracking
    pub fn define_stages(&self, stages: &[StageType]) {
        self.event_sink
            .emit(FlashEvent::StagesDefined(stages.to_vec()));
        self.reporter.define_stages(stages);
    }

    /// Begin a specific stage
    pub fn begin_stage(&self, stage_type: StageType) {
        self.event_sink.emit(FlashEvent::StageStarted(stage_type));
        self.reporter.begin_stage(stage_type);
    }

    /// Set partition stage weight for progress calculation
    pub fn set_partition_stage_weight(&self, total_bytes: u64) {
        self.event_sink
            .emit(FlashEvent::PartitionStageWeight(total_bytes));
        self.reporter.set_partition_stage_weight(total_bytes);
    }

    /// Set current partition name for display
    pub fn set_current_partition(&self, partition_name: &str) {
        self.event_sink
            .emit(FlashEvent::PartitionStarted(partition_name.to_string()));
        self.reporter.set_current_partition(partition_name);
    }

    /// Update progress (bytes written)
    #[allow(dead_code)]
    pub fn update_progress(&self, current: u64) {
        self.reporter.update_progress(current);
        self.emit_progress_snapshot();
    }

    /// Update progress with speed calculation
    pub fn update_progress_with_speed(&self, current: u64) {
        self.reporter.update_progress_with_speed(current);
        self.emit_progress_snapshot();
    }

    /// Mark current stage as completed
    pub fn complete_stage(&self) {
        let current_stage = self.reporter.current_stage();
        self.reporter.complete_stage();
        if let Some(stage) = current_stage {
            self.event_sink.emit(FlashEvent::StageCompleted(stage));
        }
        self.emit_progress_snapshot();
    }

    /// Finish progress tracking
    pub fn finish_progress(&self) {
        self.reporter.finish();
    }

    /// Update progress by percentage
    #[allow(dead_code)]
    pub fn update_progress_percent(&self, percent: u8) {
        self.reporter.update_progress_percent(percent);
        self.emit_progress_snapshot();
    }

    /// Get current progress percentage (0-100)
    #[allow(dead_code)]
    pub fn get_progress(&self) -> u8 {
        self.reporter.get_progress()
    }

    pub fn flash_finished(&self, post_action: PostAction) {
        self.event_sink.emit(FlashEvent::Finished { post_action });
    }

    fn emit_progress_snapshot(&self) {
        let snapshot = self.reporter.snapshot();
        self.event_sink.emit(FlashEvent::Progress {
            overall_percent: snapshot.precise_progress,
            stage_progress: snapshot.stage_progress,
            total: snapshot.total_bytes,
            speed: snapshot.speed,
        });
    }
}

impl Default for Logger {
    fn default() -> Self {
        Self::new()
    }
}
