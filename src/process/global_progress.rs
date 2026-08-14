//! Global progress tracking for flash operations
//!
//! Provides progress bar and stage tracking functionality using indicatif crate

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use once_cell::sync::Lazy;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Global MultiProgress instance for managing multiple progress bars
static MULTI_PROGRESS: Lazy<Arc<MultiProgress>> = Lazy::new(|| Arc::new(MultiProgress::new()));

/// Get a clone of the global MultiProgress instance
pub fn multi_progress() -> Arc<MultiProgress> {
    Arc::clone(&MULTI_PROGRESS)
}

/// Global progress tracker instance
static GLOBAL_PROGRESS: Lazy<Arc<GlobalProgress>> = Lazy::new(|| Arc::new(GlobalProgress::new()));

/// Get a clone of the global progress tracker
pub fn global_progress() -> Arc<GlobalProgress> {
    Arc::clone(&GLOBAL_PROGRESS)
}

/// TUI mode flag - when true, skip creating indicatif progress bars
static TUI_MODE: AtomicBool = AtomicBool::new(false);

/// Enable TUI mode (suppress indicatif progress bars)
pub fn set_tui_mode(enabled: bool) {
    TUI_MODE.store(enabled, Ordering::SeqCst);
}

/// Check if TUI mode is active
pub fn is_tui_mode() -> bool {
    TUI_MODE.load(Ordering::SeqCst)
}

/// Flash operation stage types
///
/// Represents different stages of the firmware flashing process
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageType {
    /// Initial stage
    Init,
    /// FEL mode DRAM initialization
    FelDram,
    /// FEL mode U-Boot download
    FelUboot,
    /// Device reconnection after FEL mode
    FelReconnect,
    /// FES mode device query
    FesQuery,
    /// Flash erasure
    FesErase,
    /// MBR (Master Boot Record) writing
    FesMbr,
    /// Partition flashing
    FesPartitions,
    /// Boot image writing
    FesBoot,
    /// Setting device mode
    FesMode,
}

impl StageType {
    /// Get human-readable name for the stage
    pub fn name(&self) -> &'static str {
        match self {
            StageType::Init => "Initializing",
            StageType::FelDram => "DRAM Init",
            StageType::FelUboot => "U-Boot Download",
            StageType::FelReconnect => "Reconnecting",
            StageType::FesQuery => "Query Device",
            StageType::FesErase => "Erasing",
            StageType::FesMbr => "Writing MBR",
            StageType::FesPartitions => "Flashing Partitions",
            StageType::FesBoot => "Writing Boot",
            StageType::FesMode => "Setting Mode",
        }
    }
}

/// Global progress tracker
///
/// Manages progress bars and tracks the completion of flash operation stages
pub struct GlobalProgress {
    progress_bar: Mutex<Option<ProgressBar>>,
    total_weight: AtomicU64,
    completed_weight: AtomicU64,
    current_stage: AtomicUsize,
    stage_progress: AtomicU64,
    total_bytes: AtomicU64,
    global_written_bytes: AtomicU64,
    stages: Mutex<Vec<StageInfo>>,
    started: AtomicUsize,
    current_partition: Mutex<String>,
    last_update_time: Mutex<Option<Instant>>,
    last_update_bytes: AtomicU64,
    current_speed: Mutex<f64>,
    precise_progress: Mutex<f64>,
}

/// Stage information for progress tracking
#[derive(Debug, Clone)]
pub struct StageInfo {
    pub stage_type: StageType,
    pub weight: u64,
    pub completed: bool,
    pub sub_total: u64,
}

/// Snapshot of progress state for TUI polling
pub struct ProgressSnapshot {
    pub precise_progress: f64,
    pub stage_progress: u64,
    pub total_bytes: u64,
    pub speed: f64,
    pub current_stage_index: usize,
    pub stages: Vec<StageInfo>,
}

impl GlobalProgress {
    /// Create a new global progress tracker
    pub fn new() -> Self {
        Self {
            progress_bar: Mutex::new(None),
            total_weight: AtomicU64::new(0),
            completed_weight: AtomicU64::new(0),
            current_stage: AtomicUsize::new(0),
            stage_progress: AtomicU64::new(0),
            total_bytes: AtomicU64::new(0),
            global_written_bytes: AtomicU64::new(0),
            stages: Mutex::new(Vec::new()),
            started: AtomicUsize::new(0),
            current_partition: Mutex::new(String::new()),
            last_update_time: Mutex::new(None),
            last_update_bytes: AtomicU64::new(0),
            current_speed: Mutex::new(0.0),
            precise_progress: Mutex::new(0.0),
        }
    }

    /// Take a snapshot of current progress state (for TUI polling)
    pub fn snapshot(&self) -> ProgressSnapshot {
        let stages = self.stages.lock().unwrap().clone();
        let speed = *self.current_speed.lock().unwrap();
        let precise = *self.precise_progress.lock().unwrap();

        ProgressSnapshot {
            precise_progress: precise,
            stage_progress: self.stage_progress.load(Ordering::SeqCst),
            total_bytes: self.total_bytes.load(Ordering::SeqCst),
            speed,
            current_stage_index: self.current_stage.load(Ordering::SeqCst),
            stages,
        }
    }

    /// Define the stages for the flash operation
    ///
    /// Sets up the progress stages with their respective weights (percentages)
    pub fn define_stages(&self, stage_types: &[StageType]) {
        let mut stages = self.stages.lock().unwrap();
        stages.clear();

        let mut cumulative_percent = 0u64;
        for stage_type in stage_types {
            let end_percent: u64 = match stage_type {
                StageType::Init => 3,
                StageType::FelDram => 5,
                StageType::FelUboot => 8,
                StageType::FelReconnect => 10,
                StageType::FesQuery => 12,
                StageType::FesErase => 14,
                StageType::FesMbr => 20,
                StageType::FesPartitions => 100,
                StageType::FesBoot => 100,
                StageType::FesMode => 100,
            };
            stages.push(StageInfo {
                stage_type: *stage_type,
                weight: end_percent.saturating_sub(cumulative_percent),
                completed: false,
                sub_total: 0,
            });
            cumulative_percent = end_percent;
        }

        self.total_weight.store(100, Ordering::SeqCst);
        self.completed_weight.store(0, Ordering::SeqCst);
        self.current_stage.store(0, Ordering::SeqCst);
        self.stage_progress.store(0, Ordering::SeqCst);
        self.total_bytes.store(0, Ordering::SeqCst);
        self.global_written_bytes.store(0, Ordering::SeqCst);
        self.last_update_bytes.store(0, Ordering::SeqCst);
        *self.current_speed.lock().unwrap() = 0.0;
        *self.precise_progress.lock().unwrap() = 0.0;
    }

    /// Set the weight for partition flashing stage based on total bytes
    pub fn set_partition_stage_weight(&self, total_bytes: u64) {
        let current = self.current_stage.load(Ordering::SeqCst);
        let mut stages = self.stages.lock().unwrap();

        if current < stages.len() && stages[current].stage_type == StageType::FesPartitions {
            let completed_weight: u64 = stages
                .iter()
                .filter(|s| s.completed)
                .map(|s| s.weight)
                .sum();

            stages[current].weight = 80;
            stages[current].sub_total = total_bytes;

            self.completed_weight
                .store(completed_weight, Ordering::SeqCst);
            self.total_bytes.store(total_bytes, Ordering::SeqCst);
            self.stage_progress.store(0, Ordering::SeqCst);
            self.global_written_bytes.store(0, Ordering::SeqCst);
        }
    }

    /// Start the global progress tracking
    pub fn start(&self) {
        if self.started.swap(1, Ordering::SeqCst) == 1 {
            return;
        }

        // In TUI mode, skip creating indicatif progress bar
        if !is_tui_mode() {
            let mp = multi_progress();
            let pb = mp.add(ProgressBar::new(100));
            pb.set_style(
                ProgressStyle::default_bar()
                    .template(
                        "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos:>3}% {msg}",
                    )
                    .unwrap()
                    .progress_chars("#>-"),
            );
            pb.enable_steady_tick(Duration::from_millis(100));

            let mut progress_bar = self.progress_bar.lock().unwrap();
            *progress_bar = Some(pb);
        }

        *self.last_update_time.lock().unwrap() = Some(Instant::now());
    }

    /// Start a specific stage
    pub fn start_stage(&self, stage_type: StageType) {
        let stages = self.stages.lock().unwrap();
        if let Some(pos) = stages.iter().position(|s| s.stage_type == stage_type) {
            self.current_stage.store(pos, Ordering::SeqCst);
            drop(stages);

            self.update_message(stage_type.name());
        }
    }

    /// Set the current partition name for display
    pub fn set_current_partition(&self, partition_name: &str) {
        let mut partition = self.current_partition.lock().unwrap();
        *partition = partition_name.to_string();
    }

    /// Update stage progress (0-100 percentage)
    pub fn update_stage_progress(&self, progress: u64) {
        let current = self.current_stage.load(Ordering::SeqCst);
        let stages = self.stages.lock().unwrap();

        if current >= stages.len() {
            return;
        }

        let stage = &stages[current];
        let stage_weight = stage.weight;
        let sub_total = stage.sub_total.max(1);

        let completed_weight = self.completed_weight.load(Ordering::SeqCst);

        drop(stages);

        let stage_percent = (progress as f64 / sub_total as f64).min(1.0);
        let percent = completed_weight as f64 + stage_percent * stage_weight as f64;

        *self.precise_progress.lock().unwrap() = percent;

        self.stage_progress.store(progress, Ordering::SeqCst);

        if let Some(pb) = self.progress_bar.lock().unwrap().as_ref() {
            pb.set_position(percent.min(100.0) as u64);
        }
    }

    /// Update stage progress with speed calculation
    pub fn update_stage_progress_with_speed(&self, progress: u64) {
        let now = Instant::now();
        let mut last_time = self.last_update_time.lock().unwrap();
        let last_bytes = self.last_update_bytes.load(Ordering::SeqCst);

        let current_stage_progress = progress;

        if let Some(last) = *last_time {
            let elapsed = now.duration_since(last).as_secs_f64();
            if elapsed > 0.0 {
                let bytes_diff = current_stage_progress.saturating_sub(last_bytes);
                let speed = bytes_diff as f64 / elapsed;
                *self.current_speed.lock().unwrap() = speed;
            }
        }

        *last_time = Some(now);
        self.last_update_bytes
            .store(current_stage_progress, Ordering::SeqCst);

        self.update_stage_progress(progress);

        self.update_progress_message();
    }

    /// Update the progress message with speed and transfer info
    pub fn update_progress_message(&self) {
        let partition = self.current_partition.lock().unwrap();
        let speed = *self.current_speed.lock().unwrap();
        let progress = self.stage_progress.load(Ordering::SeqCst);
        let total = self.total_bytes.load(Ordering::SeqCst);

        let speed_str = if speed > 1024.0 * 1024.0 {
            format!("{:.2} MB/s", speed / (1024.0 * 1024.0))
        } else if speed > 1024.0 {
            format!("{:.2} KB/s", speed / 1024.0)
        } else {
            format!("{:.0} B/s", speed)
        };

        let progress_str = if total > 0 {
            let progress_mb = progress as f64 / (1024.0 * 1024.0);
            let total_mb = total as f64 / (1024.0 * 1024.0);
            format!("{:.1}/{:.1} MB", progress_mb, total_mb)
        } else {
            String::new()
        };

        let message = if partition.is_empty() {
            format!("{} {}", speed_str, progress_str)
        } else {
            format!("[{}] {} {}", partition, speed_str, progress_str)
        };

        if let Some(pb) = self.progress_bar.lock().unwrap().as_ref() {
            pb.set_message(message);
        }
    }

    /// Mark the current stage as completed
    pub fn complete_stage(&self) {
        let current = self.current_stage.load(Ordering::SeqCst);
        let mut stages = self.stages.lock().unwrap();

        if current < stages.len() && !stages[current].completed {
            stages[current].completed = true;
            let weight = stages[current].weight;

            let completed = self.completed_weight.fetch_add(weight, Ordering::SeqCst) + weight;

            // Update precise_progress for TUI
            *self.precise_progress.lock().unwrap() = completed as f64;

            if let Some(pb) = self.progress_bar.lock().unwrap().as_ref() {
                pb.set_position(completed.min(100));
            }
        }
    }

    /// Update the progress bar message
    pub fn update_message(&self, message: &str) {
        if let Some(pb) = self.progress_bar.lock().unwrap().as_ref() {
            pb.set_message(message.to_string());
        }
    }

    /// Finish the progress tracking
    pub fn finish(&self) {
        if self.started.swap(0, Ordering::SeqCst) == 0 {
            return;
        }

        if let Some(pb) = self.progress_bar.lock().unwrap().take() {
            pb.finish_with_message("Done".to_string());
        }

        self.completed_weight.store(0, Ordering::SeqCst);
        self.current_stage.store(0, Ordering::SeqCst);
        self.stage_progress.store(0, Ordering::SeqCst);
        self.total_bytes.store(0, Ordering::SeqCst);
        self.global_written_bytes.store(0, Ordering::SeqCst);
        self.last_update_bytes.store(0, Ordering::SeqCst);
        *self.current_speed.lock().unwrap() = 0.0;
        *self.precise_progress.lock().unwrap() = 0.0;
        *self.last_update_time.lock().unwrap() = None;
        *self.current_partition.lock().unwrap() = String::new();

        let mut stages = self.stages.lock().unwrap();
        stages.clear();
    }

    /// Get current progress percentage (0-100)
    pub fn get_progress(&self) -> u8 {
        let completed = self.completed_weight.load(Ordering::SeqCst);
        let total = self.total_weight.load(Ordering::SeqCst).max(1);
        ((completed as f64 / total as f64) * 100.0) as u8
    }
}

impl Default for GlobalProgress {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_names_cover_every_variant() {
        let cases = [
            (StageType::Init, "Initializing"),
            (StageType::FelDram, "DRAM Init"),
            (StageType::FelUboot, "U-Boot Download"),
            (StageType::FelReconnect, "Reconnecting"),
            (StageType::FesQuery, "Query Device"),
            (StageType::FesErase, "Erasing"),
            (StageType::FesMbr, "Writing MBR"),
            (StageType::FesPartitions, "Flashing Partitions"),
            (StageType::FesBoot, "Writing Boot"),
            (StageType::FesMode, "Setting Mode"),
        ];

        for (stage, expected) in cases {
            assert_eq!(stage.name(), expected);
        }
    }

    #[test]
    fn definitions_reset_state_and_tolerate_nonstandard_order() {
        let progress = GlobalProgress::default();
        progress.define_stages(&[StageType::FesPartitions, StageType::Init]);

        let snapshot = progress.snapshot();
        assert_eq!(snapshot.stages.len(), 2);
        assert_eq!(snapshot.stages[0].weight, 100);
        assert_eq!(snapshot.stages[1].weight, 0);
        assert_eq!(snapshot.precise_progress, 0.0);
        assert_eq!(snapshot.stage_progress, 0);
        assert_eq!(snapshot.total_bytes, 0);
        assert_eq!(snapshot.speed, 0.0);
        assert_eq!(snapshot.current_stage_index, 0);
    }

    #[test]
    fn partition_progress_is_weighted_clamped_and_completed_once() {
        let progress = GlobalProgress::new();
        progress.define_stages(&[StageType::Init, StageType::FesMbr, StageType::FesPartitions]);

        progress.start_stage(StageType::Init);
        progress.complete_stage();
        assert_eq!(progress.get_progress(), 3);

        progress.start_stage(StageType::FesMbr);
        progress.complete_stage();
        assert_eq!(progress.get_progress(), 20);

        progress.start_stage(StageType::FesPartitions);
        progress.set_partition_stage_weight(1_000);
        progress.set_current_partition("rootfs");
        progress.update_stage_progress(250);
        let snapshot = progress.snapshot();
        assert_eq!(snapshot.precise_progress, 40.0);
        assert_eq!(snapshot.stage_progress, 250);
        assert_eq!(snapshot.total_bytes, 1_000);

        progress.update_stage_progress(2_000);
        assert_eq!(progress.snapshot().precise_progress, 100.0);
        progress.complete_stage();
        progress.complete_stage();
        assert_eq!(progress.get_progress(), 100);
    }

    #[test]
    fn irrelevant_stage_updates_are_noops() {
        let progress = GlobalProgress::new();
        progress.update_stage_progress(50);
        progress.complete_stage();
        progress.update_message("ignored");
        progress.update_progress_message();
        assert_eq!(progress.get_progress(), 0);

        progress.define_stages(&[StageType::Init, StageType::FesPartitions]);
        progress.set_partition_stage_weight(123);
        assert_eq!(progress.snapshot().total_bytes, 0);
        progress.start_stage(StageType::FesBoot);
        assert_eq!(progress.snapshot().current_stage_index, 0);
    }

    #[test]
    fn speed_and_messages_cover_byte_kibibyte_and_mebibyte_ranges() {
        let progress = GlobalProgress::new();
        progress.define_stages(&[StageType::FesPartitions]);
        progress.set_partition_stage_weight(4 * 1024 * 1024);
        progress.set_current_partition("system");
        *progress.progress_bar.lock().unwrap() = Some(ProgressBar::hidden());

        for speed in [512.0, 2048.0, 2.0 * 1024.0 * 1024.0] {
            *progress.current_speed.lock().unwrap() = speed;
            progress.update_progress_message();
        }

        *progress.last_update_time.lock().unwrap() =
            Some(Instant::now() - Duration::from_millis(100));
        progress.last_update_bytes.store(0, Ordering::SeqCst);
        progress.update_stage_progress_with_speed(1024);
        assert!(progress.snapshot().speed > 0.0);

        progress.set_current_partition("");
        progress.total_bytes.store(0, Ordering::SeqCst);
        progress.update_progress_message();
        progress.update_message("custom");
        assert_eq!(
            progress
                .progress_bar
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .message(),
            "custom"
        );
    }

    #[test]
    fn start_and_finish_are_idempotent_and_clear_reusable_state() {
        let _guard = crate::test_support::GLOBAL_STATE_TEST_LOCK.lock().unwrap();
        set_tui_mode(true);
        let progress = GlobalProgress::new();
        progress.define_stages(&[StageType::Init]);
        progress.start();
        progress.start();
        progress.update_stage_progress_with_speed(1);
        progress.complete_stage();
        progress.finish();
        progress.finish();

        let snapshot = progress.snapshot();
        assert!(snapshot.stages.is_empty());
        assert_eq!(snapshot.precise_progress, 0.0);
        assert_eq!(snapshot.stage_progress, 0);
        assert_eq!(snapshot.total_bytes, 0);
        assert_eq!(snapshot.speed, 0.0);
        assert_eq!(progress.get_progress(), 0);

        set_tui_mode(false);
        let terminal_progress = GlobalProgress::new();
        terminal_progress.start();
        assert!(terminal_progress.progress_bar.lock().unwrap().is_some());
        terminal_progress.finish();
        assert!(terminal_progress.progress_bar.lock().unwrap().is_none());
    }

    #[test]
    fn global_accessors_share_instances_and_tui_flag_round_trips() {
        let _guard = crate::test_support::GLOBAL_STATE_TEST_LOCK.lock().unwrap();
        assert!(Arc::ptr_eq(&multi_progress(), &multi_progress()));
        assert!(Arc::ptr_eq(&global_progress(), &global_progress()));
        set_tui_mode(true);
        assert!(is_tui_mode());
        set_tui_mode(false);
        assert!(!is_tui_mode());
    }
}
