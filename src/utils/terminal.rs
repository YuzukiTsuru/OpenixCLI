#![allow(dead_code)]

use colored::Colorize;
use indicatif::MultiProgress;
use log::{Level, LevelFilter, Log, Metadata, Record};
use once_cell::sync::Lazy;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
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

pub struct ProgressManager {
    multi: Arc<MultiProgress>,
    total_bytes: Arc<AtomicU64>,
    written_bytes: Arc<AtomicU64>,
}

impl ProgressManager {
    pub fn new() -> Self {
        Self {
            multi: crate::process::multi_progress(),
            total_bytes: Arc::new(AtomicU64::new(0)),
            written_bytes: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn add_total_bytes(&self, bytes: u64) {
        self.total_bytes.fetch_add(bytes, Ordering::SeqCst);
    }

    pub fn get_written_bytes(&self) -> u64 {
        self.written_bytes.load(Ordering::SeqCst)
    }
}

impl Default for ProgressManager {
    fn default() -> Self {
        Self::new()
    }
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

pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}
