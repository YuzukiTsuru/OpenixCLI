use super::global_progress::{global_progress, StageType};
use std::sync::Arc;

pub struct ProgressReporter {
    progress: Arc<super::global_progress::GlobalProgress>,
}

impl ProgressReporter {
    pub fn new() -> Self {
        Self {
            progress: global_progress(),
        }
    }

    pub fn start(&self) {
        self.progress.start();
    }

    pub fn define_stages(&self, stages: &[StageType]) {
        self.progress.define_stages(stages);
    }

    pub fn begin_stage(&self, stage_type: StageType) {
        self.progress.start_stage(stage_type);
    }

    pub fn set_partition_stage_weight(&self, total_bytes: u64) {
        self.progress.set_partition_stage_weight(total_bytes);
    }

    pub fn set_current_partition(&self, partition_name: &str) {
        self.progress.set_current_partition(partition_name);
    }

    pub fn update_progress(&self, current: u64) {
        self.progress.update_stage_progress(current);
    }

    pub fn update_progress_with_speed(&self, current: u64) {
        self.progress.update_stage_progress_with_speed(current);
    }

    pub fn update_progress_percent(&self, percent: u8) {
        self.progress.update_stage_progress(percent as u64);
    }

    pub fn complete_stage(&self) {
        self.progress.complete_stage();
    }

    pub fn finish(&self) {
        self.progress.finish();
    }

    pub fn get_progress(&self) -> u8 {
        self.progress.get_progress()
    }
}

impl Default for ProgressReporter {
    fn default() -> Self {
        Self::new()
    }
}
