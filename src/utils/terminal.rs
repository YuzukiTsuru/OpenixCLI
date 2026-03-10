//! Terminal output utilities
//!
//! Provides colored terminal output functions for logging

#![allow(dead_code)]

use colored::Colorize;
use indicatif::MultiProgress;
use log::{Level, LevelFilter, Log, Metadata, Record};
use once_cell::sync::Lazy;
use std::io::Write;
use std::sync::Arc;

/// Global MultiProgress instance
static MULTI_PROGRESS: Lazy<Arc<MultiProgress>> = Lazy::new(crate::process::multi_progress);

/// Verbose mode flag (unsafe for thread safety)
static mut VERBOSE_MODE: bool = false;

/// Set verbose mode
pub fn set_verbose(verbose: bool) {
    unsafe {
        VERBOSE_MODE = verbose;
    }
}

/// Check if verbose mode is enabled
pub fn is_verbose() -> bool {
    unsafe { VERBOSE_MODE }
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

        let level_str = self.format_level(record.level());
        let message = record.args().to_string();

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
    MULTI_PROGRESS.suspend(|| {
        println!("[{}] {}", "INFO".cyan().bold(), message);
    });
}

/// Log a success message
pub fn log_success(message: &str) {
    MULTI_PROGRESS.suspend(|| {
        println!("[{}] {}", "OKAY".green().bold(), message);
    });
}

/// Log a warning message
pub fn log_warn(message: &str) {
    MULTI_PROGRESS.suspend(|| {
        println!("[{}] {}", "WARN".yellow().bold(), message);
    });
}

/// Log an error message
pub fn log_error(message: &str) {
    MULTI_PROGRESS.suspend(|| {
        eprintln!("[{}] {}", "ERRO".red().bold(), message);
    });
}

/// Log a debug message (only if verbose mode is enabled)
pub fn log_debug(message: &str) {
    if is_verbose() {
        MULTI_PROGRESS.suspend(|| {
            println!("[{}] {}", "DEBG".blue().bold(), message);
        });
    }
}

/// Log a stage completion message
pub fn log_stage_complete(message: &str) {
    log_success(message);
}
