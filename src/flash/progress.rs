#![allow(dead_code)]

use indicatif::{ProgressBar, ProgressStyle};

pub struct ProgressManager {
    stages: Vec<Stage>,
    current_stage: usize,
    progress_bar: Option<ProgressBar>,
}

#[derive(Debug, Clone)]
pub struct Stage {
    pub name: String,
    pub progress: u8,
    pub status: StageStatus,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StageStatus {
    Pending,
    InProgress,
    Completed,
}

impl ProgressManager {
    pub fn new() -> Self {
        Self {
            stages: Vec::new(),
            current_stage: 0,
            progress_bar: None,
        }
    }

    pub fn define_stages(&mut self, stage_names: &[&str]) {
        self.stages = stage_names
            .iter()
            .map(|name| Stage {
                name: name.to_string(),
                progress: 0,
                status: StageStatus::Pending,
            })
            .collect();
    }

    pub fn start_stage(&mut self, name: &str) {
        for stage in &mut self.stages {
            if stage.name == name {
                stage.status = StageStatus::InProgress;
                break;
            }
        }

        if let Some(pos) = self.stages.iter().position(|s| s.name == name) {
            self.current_stage = pos;
        }

        if self.progress_bar.is_none() {
            let pb = ProgressBar::new(100);
            pb.set_style(
                ProgressStyle::default_bar()
                    .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos:>3}% {msg}")
                    .unwrap()
                    .progress_chars("█░"),
            );
            self.progress_bar = Some(pb);
        }

        if let Some(ref pb) = self.progress_bar {
            pb.set_message(name.to_string());
        }
    }

    pub fn update_progress(&mut self, percent: u8) {
        if self.current_stage < self.stages.len() {
            self.stages[self.current_stage].progress = percent;
        }

        if let Some(ref pb) = self.progress_bar {
            pb.set_position(percent as u64);
        }
    }

    pub fn complete_stage(&mut self) {
        if self.current_stage < self.stages.len() {
            self.stages[self.current_stage].status = StageStatus::Completed;
            self.stages[self.current_stage].progress = 100;
        }

        if let Some(ref pb) = self.progress_bar {
            pb.finish();
        }
    }

    pub fn next_stage(&mut self) {
        if self.current_stage + 1 < self.stages.len() {
            self.current_stage += 1;
            let stage_name = self.stages[self.current_stage].name.clone();
            self.start_stage(&stage_name);
        }
    }
}

impl Default for ProgressManager {
    fn default() -> Self {
        Self::new()
    }
}

pub const FEL_STAGES: &[&str] = &[
    "prepare",
    "init_dram",
    "download_uboot",
    "reconnect",
    "ready",
];

pub const FES_STAGES: &[&str] = &[
    "query_secure",
    "erase_flag",
    "query_storage",
    "mbr",
    "partitions",
    "boot",
    "set_mode",
    "complete",
];
