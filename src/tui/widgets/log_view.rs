//! Scrollable log view widget

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::tui::event::LogLevel;

/// A single log entry
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub level: LogLevel,
    pub message: String,
}

/// Log view state
pub struct LogState {
    pub entries: Vec<LogEntry>,
    pub max_entries: usize,
    pub scroll_offset: u16,
    pub auto_scroll: bool,
}

impl Default for LogState {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            max_entries: 500,
            scroll_offset: 0,
            auto_scroll: true,
        }
    }
}

impl LogState {
    pub fn push(&mut self, level: LogLevel, message: String) {
        self.entries.push(LogEntry { level, message });
        let excess = self.entries.len().saturating_sub(self.max_entries);
        if excess > 0 {
            self.entries.drain(..excess);
        }
        // auto_scroll offset is recalculated each render frame
    }
}

pub fn render(frame: &mut Frame, area: Rect, state: &mut LogState, focused: bool) {
    let border_color = if focused {
        Color::Cyan
    } else {
        Color::DarkGray
    };
    let block = Block::default()
        .title(" LOG ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let lines: Vec<Line> = state
        .entries
        .iter()
        .map(|entry| {
            let (tag, style) = match entry.level {
                LogLevel::Info => ("[INFO]", Style::default().fg(Color::Cyan)),
                LogLevel::Success => ("[OKAY]", Style::default().fg(Color::Green)),
                LogLevel::Warn => ("[WARN]", Style::default().fg(Color::Yellow)),
                LogLevel::Error => ("[ERRO]", Style::default().fg(Color::Red)),
                LogLevel::Debug => ("[DEBG]", Style::default().fg(Color::Blue)),
            };
            Line::from(vec![
                Span::styled(tag, style),
                Span::raw(" "),
                Span::raw(&entry.message),
            ])
        })
        .collect();

    // Auto-scroll: calculate actual scroll offset accounting for line wrapping
    if state.auto_scroll {
        let inner_height = area.height.saturating_sub(2); // Account for borders
        let inner_width = area.width.saturating_sub(2) as usize; // Account for borders

        // Calculate total visual lines accounting for wrapping
        let mut total_visual_lines = 0u16;
        for entry in &state.entries {
            // Each line has format: "[INFO] message"
            // Tag is 6 chars, space is 1, then message length
            let line_width = 7 + entry.message.len();
            let wrapped_lines = if inner_width > 0 {
                line_width.div_ceil(inner_width) as u16
            } else {
                1
            };
            total_visual_lines = total_visual_lines.saturating_add(wrapped_lines);
        }

        state.scroll_offset = total_visual_lines.saturating_sub(inner_height);
    }

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((state.scroll_offset, 0));

    frame.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn render_log(state: &mut LogState, width: u16, height: u16, focused: bool) -> String {
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
    fn push_enforces_capacity_including_zero_capacity() {
        let mut state = LogState {
            max_entries: 2,
            ..LogState::default()
        };
        state.push(LogLevel::Info, "one".into());
        state.push(LogLevel::Warn, "two".into());
        state.push(LogLevel::Error, "three".into());
        assert_eq!(state.entries.len(), 2);
        assert_eq!(state.entries[0].message, "two");

        state.max_entries = 0;
        state.push(LogLevel::Debug, "discarded".into());
        assert!(state.entries.is_empty());
    }

    #[test]
    fn render_maps_all_levels_and_updates_wrapped_auto_scroll() {
        let mut state = LogState::default();
        for (level, text) in [
            (LogLevel::Info, "info"),
            (LogLevel::Success, "success"),
            (LogLevel::Warn, "warn"),
            (LogLevel::Error, "error"),
            (LogLevel::Debug, "debug message that wraps across lines"),
        ] {
            state.push(level, text.into());
        }

        let text = render_log(&mut state, 24, 5, true);
        assert!(state.scroll_offset > 0);
        assert!(text.contains("[DEBG]"));

        state.auto_scroll = false;
        state.scroll_offset = 0;
        let text = render_log(&mut state, 80, 10, false);
        for tag in ["[INFO]", "[OKAY]", "[WARN]", "[ERRO]", "[DEBG]"] {
            assert!(text.contains(tag));
        }

        state.auto_scroll = true;
        let _ = render_log(&mut state, 1, 1, false);
    }
}
