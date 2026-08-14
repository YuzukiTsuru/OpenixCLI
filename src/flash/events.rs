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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn sinks_can_ignore_emit_and_share_callbacks_across_clones() {
        FlashEventSink::none().emit(FlashEvent::StageStarted(StageType::Init));
        FlashEventSink::default().emit(FlashEvent::StageCompleted(StageType::Init));

        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&events);
        let sink = FlashEventSink::from_fn(move |event| captured.lock().unwrap().push(event));
        let clone = sink.clone();
        sink.emit(FlashEvent::PartitionStageWeight(123));
        clone.emit(FlashEvent::Finished {
            post_action: PostAction::Reboot,
        });

        let events = events.lock().unwrap();
        assert!(matches!(events[0], FlashEvent::PartitionStageWeight(123)));
        assert!(matches!(
            events[1],
            FlashEvent::Finished {
                post_action: PostAction::Reboot
            }
        ));
    }
}
