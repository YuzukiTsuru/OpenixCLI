use super::terminal::{log_debug, log_error, log_info, log_stage, log_stage_complete, log_success, log_warn, multi_progress};
use indicatif::{ProgressBar, ProgressStyle};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

pub struct Logger {
    verbose: bool,
    multi: Arc<indicatif::MultiProgress>,
    progress_bar: Option<ProgressBar>,
    total_bytes: Arc<AtomicU64>,
    written_bytes: Arc<AtomicU64>,
}

impl Logger {
    pub fn new() -> Self {
        Self {
            verbose: false,
            multi: multi_progress(),
            progress_bar: None,
            total_bytes: Arc::new(AtomicU64::new(0)),
            written_bytes: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn with_verbose(verbose: bool) -> Self {
        Self {
            verbose,
            multi: multi_progress(),
            progress_bar: None,
            total_bytes: Arc::new(AtomicU64::new(0)),
            written_bytes: Arc::new(AtomicU64::new(0)),
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

    pub fn stage(&self, stage_num: usize, total_stages: usize, message: &str) {
        log_stage(stage_num, total_stages, message);
    }

    pub fn stage_complete(&self, message: &str) {
        log_stage_complete(message);
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

    #[allow(dead_code)]
    pub fn add_total_bytes(&self, bytes: u64) {
        self.total_bytes.fetch_add(bytes, Ordering::SeqCst);
        if let Some(ref pb) = self.progress_bar {
            pb.set_length(self.total_bytes.load(Ordering::SeqCst));
        }
    }

    #[allow(dead_code)]
    pub fn update_progress(&self, bytes_written: u64, message: &str) {
        self.written_bytes.store(bytes_written, Ordering::SeqCst);
        if let Some(ref pb) = self.progress_bar {
            pb.set_position(bytes_written);
            if !message.is_empty() {
                pb.set_message(message.to_string());
            }
        }
    }

    #[allow(dead_code)]
    pub fn increment_progress(&self, bytes: u64, message: &str) {
        let current = self.written_bytes.fetch_add(bytes, Ordering::SeqCst) + bytes;
        if let Some(ref pb) = self.progress_bar {
            pb.set_position(current);
            if !message.is_empty() {
                pb.set_message(message.to_string());
            }
        }
    }

    pub fn progress_update(&self, current: usize, message: &str) {
        if let Some(ref pb) = self.progress_bar {
            pb.set_position(current as u64);
            if !message.is_empty() {
                pb.set_message(message.to_string());
            }
        }
    }

    #[allow(dead_code)]
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
