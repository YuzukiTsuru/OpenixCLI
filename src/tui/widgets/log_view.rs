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
        if self.entries.len() > self.max_entries {
            self.entries.remove(0);
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
