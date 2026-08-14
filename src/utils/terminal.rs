//! Terminal output utilities
//!
//! Provides colored terminal output functions for logging

#![allow(dead_code)]

use colored::Colorize;
use indicatif::MultiProgress;
use log::{Level, LevelFilter, Log, Metadata, Record};
use once_cell::sync::Lazy;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// Global MultiProgress instance
static MULTI_PROGRESS: Lazy<Arc<MultiProgress>> = Lazy::new(crate::process::multi_progress);

/// Verbose mode flag.
static VERBOSE_MODE: AtomicBool = AtomicBool::new(false);

/// Set verbose mode
pub fn set_verbose(verbose: bool) {
    VERBOSE_MODE.store(verbose, Ordering::SeqCst);
}

/// Check if verbose mode is enabled
pub fn is_verbose() -> bool {
    VERBOSE_MODE.load(Ordering::SeqCst)
}

/// TUI log message with level and text
pub struct TuiLogMessage {
    pub level: TuiLogLevel,
    pub message: String,
}

/// Log level for TUI messages
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TuiLogLevel {
    Info,
    Success,
    Warn,
    Error,
    Debug,
}

/// Global TUI log sender - when set, log output goes to this channel instead of stdout
static TUI_LOG_SENDER: Lazy<Mutex<Option<mpsc::UnboundedSender<TuiLogMessage>>>> =
    Lazy::new(|| Mutex::new(None));

/// Set the TUI log sender channel. When set, all log_* functions send to this channel.
pub fn set_tui_log_sender(tx: Option<mpsc::UnboundedSender<TuiLogMessage>>) {
    let mut sender = TUI_LOG_SENDER.lock().unwrap();
    *sender = tx;
}

/// Send a log message to TUI channel if active, returns true if sent
fn send_to_tui(level: TuiLogLevel, message: &str) -> bool {
    let sender = TUI_LOG_SENDER.lock().unwrap();
    if let Some(ref tx) = *sender {
        tx.send(TuiLogMessage {
            level,
            message: message.to_string(),
        })
        .is_ok()
    } else {
        false
    }
}

/// Terminal logger
///
/// Implements the log crate's Log trait for colored terminal output
pub struct TermLogger {
    verbose: bool,
}

impl TermLogger {
    /// Create a new terminal logger
    pub fn new(verbose: bool) -> Self {
        Self { verbose }
    }

    /// Initialize the terminal logger
    pub fn init(verbose: bool) -> Result<(), log::SetLoggerError> {
        set_verbose(verbose);
        let logger = Box::new(Self::new(verbose));
        let level = if verbose {
            LevelFilter::Debug
        } else {
            LevelFilter::Info
        };
        log::set_boxed_logger(logger)?;
        log::set_max_level(level);
        Ok(())
    }

    /// Format log level with colors
    fn format_level(&self, level: Level) -> String {
        match level {
            Level::Error => "ERROR".red().bold().to_string(),
            Level::Warn => "WARN".yellow().bold().to_string(),
            Level::Info => "INFO".green().bold().to_string(),
            Level::Debug => "DEBUG".blue().bold().to_string(),
            Level::Trace => "TRACE".white().bold().to_string(),
        }
    }
}

impl Log for TermLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        let target = metadata.target();
        if target.starts_with("openixcli") || target.starts_with("libefex") {
            return true;
        }
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        if record.level() == Level::Debug && !self.verbose {
            return;
        }

        let message = record.args().to_string();

        // In TUI mode, send to TUI channel
        let tui_level = match record.level() {
            Level::Error => TuiLogLevel::Error,
            Level::Warn => TuiLogLevel::Warn,
            Level::Info => TuiLogLevel::Info,
            Level::Debug => TuiLogLevel::Debug,
            Level::Trace => TuiLogLevel::Debug,
        };
        if send_to_tui(tui_level, &message) {
            return;
        }

        let level_str = self.format_level(record.level());

        MULTI_PROGRESS.suspend(|| {
            if record.level() == Level::Error {
                let _ = writeln!(std::io::stderr(), "[{}] {}", level_str, message);
            } else {
                let _ = writeln!(std::io::stdout(), "[{}] {}", level_str, message);
            }
        });
    }

    fn flush(&self) {}
}

/// Log an info message
pub fn log_info(message: &str) {
    if send_to_tui(TuiLogLevel::Info, message) {
        return;
    }
    MULTI_PROGRESS.suspend(|| {
        println!("[{}] {}", "INFO".cyan().bold(), message);
    });
}

/// Log a success message
pub fn log_success(message: &str) {
    if send_to_tui(TuiLogLevel::Success, message) {
        return;
    }
    MULTI_PROGRESS.suspend(|| {
        println!("[{}] {}", "OKAY".green().bold(), message);
    });
}

/// Log a warning message
pub fn log_warn(message: &str) {
    if send_to_tui(TuiLogLevel::Warn, message) {
        return;
    }
    MULTI_PROGRESS.suspend(|| {
        println!("[{}] {}", "WARN".yellow().bold(), message);
    });
}

/// Log an error message
pub fn log_error(message: &str) {
    if send_to_tui(TuiLogLevel::Error, message) {
        return;
    }
    MULTI_PROGRESS.suspend(|| {
        eprintln!("[{}] {}", "ERRO".red().bold(), message);
    });
}

/// Log a debug message (only if verbose mode is enabled)
pub fn log_debug(message: &str) {
    if is_verbose() {
        if send_to_tui(TuiLogLevel::Debug, message) {
            return;
        }
        MULTI_PROGRESS.suspend(|| {
            println!("[{}] {}", "DEBG".blue().bold(), message);
        });
    }
}

/// Log a stage completion message
pub fn log_stage_complete(message: &str) {
    log_success(message);
}

#[cfg(test)]
mod tests {
    use super::*;
    use log::{Level, Log, Metadata, Record};

    fn drain(rx: &mut mpsc::UnboundedReceiver<TuiLogMessage>) -> Vec<TuiLogMessage> {
        let mut messages = Vec::new();
        while let Ok(message) = rx.try_recv() {
            messages.push(message);
        }
        messages
    }

    #[test]
    fn direct_log_helpers_route_all_levels_and_honor_verbose() {
        let _guard = crate::test_support::GLOBAL_STATE_TEST_LOCK.lock().unwrap();
        let (tx, mut rx) = mpsc::unbounded_channel();
        set_tui_log_sender(Some(tx));
        set_verbose(false);

        log_info("info");
        log_success("success");
        log_warn("warn");
        log_error("error");
        log_debug("hidden");
        log_stage_complete("stage");
        set_verbose(true);
        log_debug("debug");

        let messages = drain(&mut rx);
        assert_eq!(messages.len(), 6);
        assert_eq!(messages[0].level, TuiLogLevel::Info);
        assert_eq!(messages[1].level, TuiLogLevel::Success);
        assert_eq!(messages[2].level, TuiLogLevel::Warn);
        assert_eq!(messages[3].level, TuiLogLevel::Error);
        assert_eq!(messages[4].message, "stage");
        assert_eq!(messages[5].level, TuiLogLevel::Debug);
        assert_eq!(messages[5].message, "debug");
        assert!(is_verbose());

        set_tui_log_sender(None);
        set_verbose(false);
    }

    #[test]
    fn closed_tui_channel_is_not_reported_as_a_successful_send() {
        let _guard = crate::test_support::GLOBAL_STATE_TEST_LOCK.lock().unwrap();
        let (tx, rx) = mpsc::unbounded_channel();
        drop(rx);
        set_tui_log_sender(Some(tx));
        assert!(!send_to_tui(TuiLogLevel::Info, "orphaned"));
        set_tui_log_sender(None);
    }

    #[test]
    fn term_logger_filters_targets_debug_and_maps_record_levels() {
        let _guard = crate::test_support::GLOBAL_STATE_TEST_LOCK.lock().unwrap();
        let logger = TermLogger::new(false);
        assert!(logger.enabled(
            &Metadata::builder()
                .level(Level::Debug)
                .target("openixcli::x")
                .build()
        ));
        assert!(logger.enabled(
            &Metadata::builder()
                .level(Level::Trace)
                .target("libefex::x")
                .build()
        ));
        assert!(logger.enabled(
            &Metadata::builder()
                .level(Level::Info)
                .target("dependency")
                .build()
        ));
        assert!(!logger.enabled(
            &Metadata::builder()
                .level(Level::Debug)
                .target("dependency")
                .build()
        ));

        for level in [
            Level::Error,
            Level::Warn,
            Level::Info,
            Level::Debug,
            Level::Trace,
        ] {
            assert!(!logger.format_level(level).is_empty());
        }

        let (tx, mut rx) = mpsc::unbounded_channel();
        set_tui_log_sender(Some(tx));
        for level in [
            Level::Error,
            Level::Warn,
            Level::Info,
            Level::Debug,
            Level::Trace,
        ] {
            let args = format_args!("{level}");
            let record = Record::builder()
                .args(args)
                .level(level)
                .target("openixcli::test")
                .build();
            logger.log(&record);
        }
        logger.flush();

        let messages = drain(&mut rx);
        assert_eq!(messages.len(), 4);
        assert_eq!(messages[0].level, TuiLogLevel::Error);
        assert_eq!(messages[1].level, TuiLogLevel::Warn);
        assert_eq!(messages[2].level, TuiLogLevel::Info);
        assert_eq!(messages[3].level, TuiLogLevel::Debug);

        set_tui_log_sender(None);
    }

    #[test]
    fn logger_initialization_sets_level_and_rejects_a_second_global_logger() {
        let _guard = crate::test_support::GLOBAL_STATE_TEST_LOCK.lock().unwrap();
        assert!(TermLogger::init(false).is_ok());
        assert_eq!(log::max_level(), LevelFilter::Info);
        assert!(TermLogger::init(true).is_err());
        set_verbose(false);
    }

    #[test]
    fn terminal_fallback_writes_all_levels_without_a_tui_receiver() {
        let _guard = crate::test_support::GLOBAL_STATE_TEST_LOCK.lock().unwrap();
        set_tui_log_sender(None);
        assert!(!send_to_tui(TuiLogLevel::Info, "no receiver"));
        set_verbose(true);
        log_info("info fallback");
        log_success("success fallback");
        log_warn("warning fallback");
        log_error("error fallback");
        log_debug("debug fallback");
        log_stage_complete("stage fallback");

        let logger = TermLogger::new(true);
        for level in [
            Level::Error,
            Level::Warn,
            Level::Info,
            Level::Debug,
            Level::Trace,
        ] {
            logger.log(
                &Record::builder()
                    .args(format_args!("fallback"))
                    .level(level)
                    .target("openixcli::fallback")
                    .build(),
            );
        }
        logger.log(
            &Record::builder()
                .args(format_args!("filtered"))
                .level(Level::Debug)
                .target("dependency")
                .build(),
        );
        set_verbose(false);
    }
}
