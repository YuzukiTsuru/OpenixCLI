//! TUI module for interactive firmware flashing
//!
//! Provides a single-page terminal UI with device scanning, firmware loading,
//! option configuration, and flash progress tracking.

mod app;
mod bridge;
mod event;
mod ui;
mod widgets;

pub use app::run;
