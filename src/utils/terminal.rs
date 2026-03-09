#![allow(dead_code)]

use colored::Colorize;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use log::{Level, LevelFilter, Log, Metadata, Record};
use once_cell::sync::Lazy;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

static MULTI_PROGRESS: Lazy<Arc<MultiProgress>> = Lazy::new(|| Arc::new(MultiProgress::new()));

static mut VERBOSE_MODE: bool = false;

pub fn multi_progress() -> Arc<MultiProgress> {
    Arc::clone(&MULTI_PROGRESS)
}

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
    progress_bar: Option<ProgressBar>,
    total_bytes: Arc<AtomicU64>,
    written_bytes: Arc<AtomicU64>,
}

impl ProgressManager {
    pub fn new() -> Self {
        Self {
            multi: multi_progress(),
            progress_bar: None,
            total_bytes: Arc::new(AtomicU64::new(0)),
            written_bytes: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn start_progress(&mut self, total: u64, message: &str) {
        self.total_bytes.store(total, Ordering::SeqCst);
        self.written_bytes.store(0, Ordering::SeqCst);

        let pb = self.multi.add(ProgressBar::new(total));
        pb.set_style(
            ProgressStyle::default_bar()
                .template("[{elapsed_precise}] {bar:40.cyan/blue} {bytes:>10}/{total_bytes:<10} {bytes_per_sec:>12} {msg}")
                .unwrap()
                .progress_chars("█░"),
        );
        pb.set_message(message.to_string());
        self.progress_bar = Some(pb);
    }

    pub fn add_total_bytes(&self, bytes: u64) {
        self.total_bytes.fetch_add(bytes, Ordering::SeqCst);
        if let Some(ref pb) = self.progress_bar {
            pb.set_length(self.total_bytes.load(Ordering::SeqCst));
        }
    }

    pub fn update_progress(&self, bytes_written: u64, message: &str) {
        self.written_bytes.store(bytes_written, Ordering::SeqCst);
        if let Some(ref pb) = self.progress_bar {
            pb.set_position(bytes_written);
            if !message.is_empty() {
                pb.set_message(message.to_string());
            }
        }
    }

    pub fn increment_progress(&self, bytes: u64, message: &str) {
        let current = self.written_bytes.fetch_add(bytes, Ordering::SeqCst) + bytes;
        if let Some(ref pb) = self.progress_bar {
            pb.set_position(current);
            if !message.is_empty() {
                pb.set_message(message.to_string());
            }
        }
    }

    pub fn set_message(&self, message: &str) {
        if let Some(ref pb) = self.progress_bar {
            pb.set_message(message.to_string());
        }
    }

    pub fn get_written_bytes(&self) -> u64 {
        self.written_bytes.load(Ordering::SeqCst)
    }

    pub fn finish_progress(&mut self) {
        if let Some(pb) = self.progress_bar.take() {
            pb.finish();
        }
    }

    pub fn finish_with_message(&mut self, message: &str) {
        if let Some(pb) = self.progress_bar.take() {
            pb.finish_with_message(message.to_string());
        }
    }

    pub fn is_active(&self) -> bool {
        self.progress_bar.is_some()
    }
}

impl Default for ProgressManager {
    fn default() -> Self {
        Self::new()
    }
}

pub fn log_info(message: &str) {
    MULTI_PROGRESS.suspend(|| {
        println!("[{}] {}", "INFO".green().bold(), message);
    });
}

pub fn log_success(message: &str) {
    MULTI_PROGRESS.suspend(|| {
        println!("[{}] {}", "SUCCESS".green().bold(), message);
    });
}

pub fn log_warn(message: &str) {
    MULTI_PROGRESS.suspend(|| {
        println!("[{}] {}", "WARN".yellow().bold(), message);
    });
}

pub fn log_error(message: &str) {
    MULTI_PROGRESS.suspend(|| {
        eprintln!("[{}] {}", "ERROR".red().bold(), message);
    });
}

pub fn log_debug(message: &str) {
    if is_verbose() {
        MULTI_PROGRESS.suspend(|| {
            println!("[{}] {}", "DEBUG".blue().bold(), message);
        });
    }
}

pub fn log_stage(stage_num: usize, total_stages: usize, message: &str) {
    MULTI_PROGRESS.suspend(|| {
        println!();
        println!(
            "{} {}/{}: {}",
            "Stage".cyan().bold(),
            stage_num,
            total_stages,
            message.white().bold()
        );
    });
}

pub fn log_stage_complete(message: &str) {
    MULTI_PROGRESS.suspend(|| {
        println!("  {} {}", "✓".green(), message);
    });
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
