#![allow(dead_code)]

use colored::Colorize;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::SystemTime;

pub struct Logger {
    verbose: bool,
    multi: Arc<MultiProgress>,
    progress_bar: Option<ProgressBar>,
    total_bytes: Arc<AtomicU64>,
    written_bytes: Arc<AtomicU64>,
}

impl Logger {
    pub fn new() -> Self {
        Self {
            verbose: false,
            multi: Arc::new(MultiProgress::new()),
            progress_bar: None,
            total_bytes: Arc::new(AtomicU64::new(0)),
            written_bytes: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn with_verbose(verbose: bool) -> Self {
        Self {
            verbose,
            multi: Arc::new(MultiProgress::new()),
            progress_bar: None,
            total_bytes: Arc::new(AtomicU64::new(0)),
            written_bytes: Arc::new(AtomicU64::new(0)),
        }
    }

    #[allow(dead_code)]
    fn timestamp(&self) -> String {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default();
        let secs = now.as_secs();
        let datetime = chrono::DateTime::from_timestamp(secs as i64, 0)
            .unwrap_or(chrono::DateTime::UNIX_EPOCH);
        datetime.format("%H:%M:%S").to_string()
    }

    pub fn info(&self, message: &str) {
        self.multi.suspend(|| {
            println!("[{}] {}", "INFO".green().bold(), message);
        });
    }

    pub fn success(&self, message: &str) {
        self.multi.suspend(|| {
            println!("[{}] {}", "SUCCESS".green().bold(), message);
        });
    }

    pub fn warn(&self, message: &str) {
        self.multi.suspend(|| {
            println!("[{}] {}", "WARN".yellow().bold(), message);
        });
    }

    pub fn error(&self, message: &str) {
        self.multi.suspend(|| {
            eprintln!("[{}] {}", "ERROR".red().bold(), message);
        });
    }

    pub fn debug(&self, message: &str) {
        if self.verbose {
            self.multi.suspend(|| {
                println!("[{}] {}", "DEBUG".blue().bold(), message);
            });
        }
    }

    pub fn stage(&self, stage_num: usize, total_stages: usize, message: &str) {
        self.multi.suspend(|| {
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

    pub fn stage_complete(&self, message: &str) {
        self.multi.suspend(|| {
            println!("  {} {}", "✓".green(), message);
        });
    }

    pub fn start_global_progress(&mut self, total: u64, message: &str) {
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
            pb.set_message(message.to_string());
        }
    }

    pub fn increment_progress(&self, bytes: u64, message: &str) {
        let current = self.written_bytes.fetch_add(bytes, Ordering::SeqCst) + bytes;
        if let Some(ref pb) = self.progress_bar {
            pb.set_position(current);
            pb.set_message(message.to_string());
        }
    }

    pub fn progress_update(&self, current: usize, message: &str) {
        if let Some(ref pb) = self.progress_bar {
            pb.set_position(current as u64);
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
}

impl Default for Logger {
    fn default() -> Self {
        Self::new()
    }
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
