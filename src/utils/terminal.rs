#![allow(dead_code)]

use colored::Colorize;
use indicatif::MultiProgress;
use log::{Level, LevelFilter, Log, Metadata, Record};
use once_cell::sync::Lazy;
use std::io::Write;
use std::sync::Arc;

static MULTI_PROGRESS: Lazy<Arc<MultiProgress>> = Lazy::new(|| crate::process::multi_progress());

static mut VERBOSE_MODE: bool = false;

pub fn set_verbose(verbose: bool) {
    unsafe {
        VERBOSE_MODE = verbose;
    }
}

pub fn is_verbose() -> bool {
    unsafe { VERBOSE_MODE }
}

pub struct TermLogger {
    verbose: bool,
}

impl TermLogger {
    pub fn new(verbose: bool) -> Self {
        Self { verbose }
    }

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

pub fn log_info(message: &str) {
    MULTI_PROGRESS.suspend(|| {
        println!("[{}] {}", "INFO".cyan().bold(), message);
    });
}

pub fn log_success(message: &str) {
    MULTI_PROGRESS.suspend(|| {
        println!("[{}] {}", "OKAY".green().bold(), message);
    });
}

pub fn log_warn(message: &str) {
    MULTI_PROGRESS.suspend(|| {
        println!("[{}] {}", "WARN".yellow().bold(), message);
    });
}

pub fn log_error(message: &str) {
    MULTI_PROGRESS.suspend(|| {
        eprintln!("[{}] {}", "ERRO".red().bold(), message);
    });
}

pub fn log_debug(message: &str) {
    if is_verbose() {
        MULTI_PROGRESS.suspend(|| {
            println!("[{}] {}", "DEBG".blue().bold(), message);
        });
    }
}

pub fn log_stage_complete(message: &str) {
    log_success(message);
}
