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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::global_progress::set_tui_mode;
    use crate::utils::terminal::{set_tui_log_sender, set_verbose, TuiLogLevel};
    use std::sync::Mutex;
    use tokio::sync::mpsc;

    #[test]
    fn constructors_and_terminal_output_paths_cover_verbose_modes() {
        let _guard = crate::test_support::GLOBAL_STATE_TEST_LOCK.lock().unwrap();
        let default_logger = Logger::default();
        assert!(!default_logger.verbose);
        assert!(default_logger.terminal_output);
        let quiet_logger = Logger::with_verbose(false);
        assert!(!quiet_logger.verbose);

        let (tx, mut rx) = mpsc::unbounded_channel();
        set_tui_log_sender(Some(tx));
        set_verbose(true);
        default_logger.info("info");
        default_logger.success("success");
        default_logger.warn("warn");
        default_logger.error("error");
        default_logger.debug("hidden");
        default_logger.stage_complete("stage");
        Logger::with_verbose(true).debug("debug");

        let mut levels = Vec::new();
        while let Ok(message) = rx.try_recv() {
            levels.push(message.level);
        }
        assert_eq!(
            levels,
            vec![
                TuiLogLevel::Info,
                TuiLogLevel::Success,
                TuiLogLevel::Warn,
                TuiLogLevel::Error,
                TuiLogLevel::Success,
                TuiLogLevel::Debug,
            ]
        );
        set_tui_log_sender(None);
        set_verbose(false);
    }

    #[test]
    fn event_logger_emits_every_pipeline_event_and_progress_snapshot() {
        let _guard = crate::test_support::GLOBAL_STATE_TEST_LOCK.lock().unwrap();
        set_tui_mode(true);
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&events);
        let logger = Logger::for_events(
            true,
            FlashEventSink::from_fn(move |event| captured.lock().unwrap().push(event)),
        );

        logger.finish_progress();
        logger.define_stages(&[StageType::Init, StageType::FesPartitions]);
        logger.start_global_progress();
        logger.begin_stage(StageType::Init);
        logger.info("info");
        logger.success("success");
        logger.warn("warn");
        logger.error("error");
        logger.debug("debug");
        logger.stage_complete("stage");
        logger.update_progress_percent(100);
        logger.complete_stage();
        logger.begin_stage(StageType::FesPartitions);
        logger.set_partition_stage_weight(1_000);
        logger.set_current_partition("rootfs");
        logger.update_progress(250);
        logger.update_progress_with_speed(500);
        assert_eq!(logger.get_progress(), 3);
        logger.complete_stage();
        logger.flash_finished(PostAction::Shutdown);
        logger.finish_progress();

        let events = events.lock().unwrap();
        for level in [
            FlashLogLevel::Info,
            FlashLogLevel::Success,
            FlashLogLevel::Warn,
            FlashLogLevel::Error,
            FlashLogLevel::Debug,
        ] {
            assert!(events.iter().any(
                |event| matches!(event, FlashEvent::Log { level: actual, .. } if *actual == level)
            ));
        }
        assert!(events
            .iter()
            .any(|event| matches!(event, FlashEvent::StagesDefined(stages) if stages.len() == 2)));
        assert!(events
            .iter()
            .any(|event| matches!(event, FlashEvent::StageStarted(StageType::FesPartitions))));
        assert!(events
            .iter()
            .any(|event| matches!(event, FlashEvent::StageCompleted(StageType::Init))));
        assert!(events
            .iter()
            .any(|event| matches!(event, FlashEvent::PartitionStageWeight(1_000))));
        assert!(events
            .iter()
            .any(|event| matches!(event, FlashEvent::PartitionStarted(name) if name == "rootfs")));
        assert!(events.iter().any(|event| matches!(
            event,
            FlashEvent::Progress {
                stage_progress: 500,
                total: 1_000,
                ..
            }
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            FlashEvent::Finished {
                post_action: PostAction::Shutdown
            }
        )));

        set_tui_mode(false);
    }

    #[test]
    fn non_verbose_event_logger_suppresses_debug() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&events);
        let logger = Logger::for_events(
            false,
            FlashEventSink::from_fn(move |event| captured.lock().unwrap().push(event)),
        );
        logger.debug("hidden");
        assert!(events.lock().unwrap().is_empty());
    }
}
