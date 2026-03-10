use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use once_cell::sync::Lazy;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

static MULTI_PROGRESS: Lazy<Arc<MultiProgress>> = Lazy::new(|| Arc::new(MultiProgress::new()));

pub fn multi_progress() -> Arc<MultiProgress> {
    Arc::clone(&MULTI_PROGRESS)
}

static GLOBAL_PROGRESS: Lazy<Arc<GlobalProgress>> = Lazy::new(|| Arc::new(GlobalProgress::new()));

pub fn global_progress() -> Arc<GlobalProgress> {
    Arc::clone(&GLOBAL_PROGRESS)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageType {
    Init,
    FelDram,
    FelUboot,
    FelReconnect,
    FesQuery,
    FesErase,
    FesMbr,
    FesPartitions,
    FesBoot,
    FesMode,
}

impl StageType {
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

#[derive(Debug, Clone)]
pub struct StageInfo {
    pub stage_type: StageType,
    pub weight: u64,
    pub completed: bool,
    pub sub_total: u64,
}

impl GlobalProgress {
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

    pub fn define_stages(&self, stage_types: &[StageType]) {
        let mut stages = self.stages.lock().unwrap();
        stages.clear();

        let mut cumulative_percent = 0u64;
        for stage_type in stage_types {
            let end_percent = match stage_type {
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
                weight: end_percent - cumulative_percent,
                completed: false,
                sub_total: 0,
            });
            cumulative_percent = end_percent;
        }

        self.total_weight.store(100, Ordering::SeqCst);
        self.completed_weight.store(0, Ordering::SeqCst);
        self.current_stage.store(0, Ordering::SeqCst);
    }

    pub fn set_partition_stage_weight(&self, total_bytes: u64) {
        let current = self.current_stage.load(Ordering::SeqCst);
        let mut stages = self.stages.lock().unwrap();

        if current < stages.len() && stages[current].stage_type == StageType::FesPartitions {
            let completed_weight: u64 = stages.iter()
                .filter(|s| s.completed)
                .map(|s| s.weight)
                .sum();

            stages[current].weight = 80;
            stages[current].sub_total = total_bytes;

            self.completed_weight.store(completed_weight, Ordering::SeqCst);
            self.total_bytes.store(total_bytes, Ordering::SeqCst);
            self.stage_progress.store(0, Ordering::SeqCst);
            self.global_written_bytes.store(0, Ordering::SeqCst);
        }
    }

    pub fn start(&self) {
        if self.started.swap(1, Ordering::SeqCst) == 1 {
            return;
        }

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

        *self.last_update_time.lock().unwrap() = Some(Instant::now());
    }

    pub fn start_stage(&self, stage_type: StageType) {
        let stages = self.stages.lock().unwrap();
        if let Some(pos) = stages.iter().position(|s| s.stage_type == stage_type) {
            self.current_stage.store(pos, Ordering::SeqCst);
            drop(stages);

            self.update_message(stage_type.name());
        }
    }

    pub fn set_current_partition(&self, partition_name: &str) {
        let mut partition = self.current_partition.lock().unwrap();
        *partition = partition_name.to_string();
    }

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

    pub fn complete_stage(&self) {
        let current = self.current_stage.load(Ordering::SeqCst);
        let mut stages = self.stages.lock().unwrap();

        if current < stages.len() {
            stages[current].completed = true;
            let weight = stages[current].weight;

            let completed = self.completed_weight.fetch_add(weight, Ordering::SeqCst) + weight;

            if let Some(pb) = self.progress_bar.lock().unwrap().as_ref() {
                pb.set_position(completed.min(100));
            }
        }
    }

    pub fn update_message(&self, message: &str) {
        if let Some(pb) = self.progress_bar.lock().unwrap().as_ref() {
            pb.set_message(message.to_string());
        }
    }

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
        *self.current_partition.lock().unwrap() = String::new();

        let mut stages = self.stages.lock().unwrap();
        stages.clear();
    }

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
