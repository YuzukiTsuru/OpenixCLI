//! Progress display widget

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Gauge, Paragraph};

use crate::process::StageType;

/// Progress state tracked by the TUI
#[derive(Default)]
pub struct ProgressState {
    pub overall_percent: f64,
    pub stage_progress: u64,
    pub stage_total: u64,
    pub speed: f64,
    pub current_partition: String,
    pub current_stage: Option<StageType>,
    pub completed_stages: Vec<StageType>,
    pub all_stages: Vec<StageType>,
    pub stage_index: usize,
    pub elapsed_secs: u64,
    pub finished: bool,
    pub error: Option<String>,
}

impl ProgressState {
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

pub fn render(frame: &mut Frame, area: Rect, state: &ProgressState, focused: bool) {
    let border_color = if focused {
        Color::Cyan
    } else {
        Color::DarkGray
    };
    let block = Block::default()
        .title(" PROGRESS ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // If no stages defined, show waiting message
    if state.all_stages.is_empty() && state.error.is_none() && !state.finished {
        let msg = if state.overall_percent > 0.0 {
            "Ready to flash"
        } else {
            "Waiting for task..."
        };
        let paragraph = Paragraph::new(Line::from(Span::styled(
            msg,
            Style::default().fg(Color::DarkGray),
        )))
        .alignment(Alignment::Center);
        let centered = center_vertical(inner, 1);
        frame.render_widget(paragraph, centered);
        return;
    }

    // Error state
    if let Some(ref err) = state.error {
        let chunks = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(2),
            Constraint::Min(0),
        ])
        .split(inner);

        // Error gauge
        let overall_percent = state.overall_percent.clamp(0.0, 100.0);
        let gauge = Gauge::default()
            .gauge_style(Style::default().fg(Color::Red).bg(Color::DarkGray))
            .percent(overall_percent as u16)
            .label(format!("{}% ERR", overall_percent as u16));
        frame.render_widget(gauge, chunks[0]);

        let err_line = Line::from(vec![
            Span::styled("FAILED: ", Style::default().fg(Color::Red).bold()),
            Span::raw(err.as_str()),
        ]);
        frame.render_widget(Paragraph::new(err_line), chunks[1]);
        return;
    }

    // Normal/finished progress
    let chunks = Layout::vertical([
        Constraint::Length(1), // Stage label + speed/elapsed
        Constraint::Length(1), // Overall gauge
        Constraint::Length(1), // Spacer
        Constraint::Length(1), // Partition label
        Constraint::Length(1), // Partition gauge
        Constraint::Length(1), // Spacer
        Constraint::Min(1),    // Stages pipeline
    ])
    .split(inner);

    // Stage label with speed and elapsed on the same line
    let stage_label = if state.finished {
        "COMPLETE".to_string()
    } else if let Some(ref stage) = state.current_stage {
        let total = state.all_stages.len();
        format!(
            "Stage: {} [{}/{}]",
            stage.name(),
            state.stage_index + 1,
            total
        )
    } else {
        "Initializing...".to_string()
    };

    let speed_str = format_speed(state.speed);
    let elapsed_str = format_duration(state.elapsed_secs);

    let stage_line = if state.finished {
        Line::from(vec![
            Span::styled("COMPLETE", Style::default().fg(Color::Green).bold()),
            Span::raw("    "),
            Span::styled(elapsed_str, Style::default().fg(Color::White)),
        ])
    } else {
        Line::from(vec![
            Span::styled(stage_label, Style::default().fg(Color::White).bold()),
            Span::raw("    "),
            Span::styled(speed_str, Style::default().fg(Color::White)),
            Span::raw("   "),
            Span::styled(elapsed_str, Style::default().fg(Color::White)),
        ])
    };
    frame.render_widget(Paragraph::new(stage_line), chunks[0]);

    // Overall gauge
    let gauge_color = if state.finished {
        Color::Green
    } else {
        Color::White
    };
    let overall_percent = state.overall_percent.clamp(0.0, 100.0);
    let gauge = Gauge::default()
        .gauge_style(Style::default().fg(gauge_color).bg(Color::DarkGray))
        .percent(overall_percent as u16)
        .label(format!("{}%", overall_percent as u16));
    frame.render_widget(gauge, chunks[1]);

    // Partition progress
    if !state.current_partition.is_empty() && state.stage_total > 0 {
        let part_progress_mb = state.stage_progress as f64 / (1024.0 * 1024.0);
        let part_total_mb = state.stage_total as f64 / (1024.0 * 1024.0);
        let part_pct =
            ((state.stage_progress as f64 / state.stage_total as f64) * 100.0).min(100.0);

        let part_label = format!(
            "Partition: {}  {:.1}/{:.1} MB",
            state.current_partition, part_progress_mb, part_total_mb
        );
        frame.render_widget(
            Paragraph::new(Span::styled(part_label, Style::default().fg(Color::White))),
            chunks[3],
        );

        let part_gauge = Gauge::default()
            .gauge_style(Style::default().fg(Color::White).bg(Color::DarkGray))
            .percent(part_pct as u16);
        frame.render_widget(part_gauge, chunks[4]);
    }

    // Stages pipeline
    if !state.all_stages.is_empty() {
        let mut spans = Vec::new();
        spans.push(Span::raw(" Stages: "));
        for (i, stage) in state.all_stages.iter().enumerate() {
            let short_name = stage_short_name(stage);
            let style = if state.completed_stages.contains(stage) {
                Style::default().fg(Color::Green)
            } else if state.current_stage.as_ref() == Some(stage) {
                Style::default().fg(Color::Yellow).bold()
            } else {
                Style::default().fg(Color::DarkGray)
            };

            if state.current_stage.as_ref() == Some(stage) {
                spans.push(Span::styled(format!("[{}]", short_name), style));
            } else {
                spans.push(Span::styled(short_name.to_string(), style));
            }

            if i < state.all_stages.len() - 1 {
                spans.push(Span::styled(" > ", Style::default().fg(Color::DarkGray)));
            }
        }
        frame.render_widget(
            Paragraph::new(Line::from(spans)).wrap(ratatui::widgets::Wrap { trim: false }),
            chunks[6],
        );
    }
}

fn stage_short_name(stage: &StageType) -> &'static str {
    match stage {
        StageType::Init => "Init",
        StageType::FelDram => "DRAM",
        StageType::FelUboot => "UBoot",
        StageType::FelReconnect => "Reconnect",
        StageType::FesQuery => "Query",
        StageType::FesErase => "Erase",
        StageType::FesMbr => "MBR",
        StageType::FesPartitions => "Partitions",
        StageType::FesBoot => "Boot",
        StageType::FesMode => "Mode",
    }
}

fn format_speed(bytes_per_sec: f64) -> String {
    if bytes_per_sec > 1024.0 * 1024.0 {
        format!("{:.1} MB/s", bytes_per_sec / (1024.0 * 1024.0))
    } else if bytes_per_sec > 1024.0 {
        format!("{:.1} KB/s", bytes_per_sec / 1024.0)
    } else if bytes_per_sec > 0.0 {
        format!("{:.0} B/s", bytes_per_sec)
    } else {
        String::new()
    }
}

fn format_duration(secs: u64) -> String {
    let m = secs / 60;
    let s = secs % 60;
    format!("{:02}:{:02} elapsed", m, s)
}

fn center_vertical(area: Rect, height: u16) -> Rect {
    let offset = area.height.saturating_sub(height) / 2;
    Rect {
        x: area.x,
        y: area.y + offset,
        width: area.width,
        height: height.min(area.height),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn render_progress(state: &ProgressState, width: u16, height: u16, focused: bool) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render(frame, frame.area(), state, focused))
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn formatting_helpers_cover_all_stages_and_unit_boundaries() {
        let stages = [
            (StageType::Init, "Init"),
            (StageType::FelDram, "DRAM"),
            (StageType::FelUboot, "UBoot"),
            (StageType::FelReconnect, "Reconnect"),
            (StageType::FesQuery, "Query"),
            (StageType::FesErase, "Erase"),
            (StageType::FesMbr, "MBR"),
            (StageType::FesPartitions, "Partitions"),
            (StageType::FesBoot, "Boot"),
            (StageType::FesMode, "Mode"),
        ];
        for (stage, expected) in stages {
            assert_eq!(stage_short_name(&stage), expected);
        }
        assert_eq!(format_speed(0.0), "");
        assert_eq!(format_speed(512.0), "512 B/s");
        assert_eq!(format_speed(2048.0), "2.0 KB/s");
        assert_eq!(format_speed(2.0 * 1024.0 * 1024.0), "2.0 MB/s");
        assert_eq!(format_duration(125), "02:05 elapsed");
        assert_eq!(
            center_vertical(Rect::new(1, 2, 3, 4), 2),
            Rect::new(1, 3, 3, 2)
        );
        assert_eq!(
            center_vertical(Rect::new(1, 2, 3, 1), 9),
            Rect::new(1, 2, 3, 1)
        );
    }

    #[test]
    fn reset_restores_every_progress_field() {
        let mut state = ProgressState {
            overall_percent: 50.0,
            stage_progress: 1,
            stage_total: 2,
            speed: 3.0,
            current_partition: "rootfs".into(),
            current_stage: Some(StageType::FesPartitions),
            completed_stages: vec![StageType::Init],
            all_stages: vec![StageType::Init],
            stage_index: 1,
            elapsed_secs: 4,
            finished: true,
            error: Some("error".into()),
        };
        state.reset();
        assert_eq!(state.overall_percent, 0.0);
        assert!(state.all_stages.is_empty());
        assert!(state.error.is_none());
        assert!(!state.finished);
    }

    #[test]
    fn rendering_covers_waiting_ready_error_active_and_finished_states() {
        let waiting = ProgressState::default();
        assert!(render_progress(&waiting, 60, 10, false).contains("Waiting for task"));

        let ready = ProgressState {
            overall_percent: 1.0,
            ..ProgressState::default()
        };
        assert!(render_progress(&ready, 60, 10, true).contains("Ready to flash"));

        let error = ProgressState {
            overall_percent: 150.0,
            error: Some("transport failed".into()),
            ..ProgressState::default()
        };
        let error_text = render_progress(&error, 60, 10, false);
        assert!(error_text.contains("100% ERR"));
        assert!(error_text.contains("transport failed"));

        let active = ProgressState {
            overall_percent: 45.0,
            stage_progress: 3 * 1024 * 1024,
            stage_total: 2 * 1024 * 1024,
            speed: 2.0 * 1024.0 * 1024.0,
            current_partition: "system".into(),
            current_stage: Some(StageType::FesPartitions),
            completed_stages: vec![StageType::Init],
            all_stages: vec![
                StageType::Init,
                StageType::FesPartitions,
                StageType::FesBoot,
            ],
            stage_index: 1,
            elapsed_secs: 65,
            ..ProgressState::default()
        };
        let active_text = render_progress(&active, 80, 12, false);
        assert!(active_text.contains("Flashing Partitions"));
        assert!(active_text.contains("Partition: system"));
        assert!(active_text.contains("[Partitions]"));

        let initializing = ProgressState {
            all_stages: vec![StageType::Init, StageType::FesQuery],
            ..ProgressState::default()
        };
        let initializing_text = render_progress(&initializing, 60, 10, false);
        assert!(initializing_text.contains("Initializing"));
        assert!(initializing_text.contains("Init"));
        assert!(initializing_text.contains("Query"));

        let finished = ProgressState {
            overall_percent: 100.0,
            finished: true,
            all_stages: vec![StageType::Init],
            elapsed_secs: 5,
            ..ProgressState::default()
        };
        assert!(render_progress(&finished, 60, 10, false).contains("COMPLETE"));

        let finished_without_pipeline = ProgressState {
            overall_percent: 100.0,
            finished: true,
            ..ProgressState::default()
        };
        assert!(render_progress(&finished_without_pipeline, 60, 10, false).contains("COMPLETE"));
    }
}
