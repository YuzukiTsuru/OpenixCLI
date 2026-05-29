//! Flash operation events emitted to CLI and TUI frontends.

use std::sync::Arc;

use crate::flash::PostAction;
use crate::process::StageType;

/// Severity for flash log events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlashLogLevel {
    Info,
    Success,
    Warn,
    Error,
    Debug,
}

/// Event stream produced by the flash pipeline.
#[derive(Debug, Clone)]
pub enum FlashEvent {
    Log {
        level: FlashLogLevel,
        message: String,
    },
    StagesDefined(Vec<StageType>),
    StageStarted(StageType),
    StageCompleted(StageType),
    PartitionStageWeight(u64),
    PartitionStarted(String),
    Progress {
        overall_percent: f64,
        stage_progress: u64,
        total: u64,
        speed: f64,
    },
    Finished {
        post_action: PostAction,
    },
}

type EventCallback = Arc<dyn Fn(FlashEvent) + Send + Sync + 'static>;

/// Optional event sink passed into lower-level flash code.
#[derive(Clone, Default)]
pub struct FlashEventSink {
    callback: Option<EventCallback>,
}

impl FlashEventSink {
    pub fn none() -> Self {
        Self { callback: None }
    }

    pub fn from_fn(callback: impl Fn(FlashEvent) + Send + Sync + 'static) -> Self {
        Self {
            callback: Some(Arc::new(callback)),
        }
    }

    pub fn emit(&self, event: FlashEvent) {
        if let Some(callback) = &self.callback {
            callback(event);
        }
    }
}
