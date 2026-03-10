//! Device list widget

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::tui::event::DeviceInfo;

pub fn render(
    frame: &mut Frame,
    area: Rect,
    devices: &[DeviceInfo],
    selected: usize,
    scroll_offset: usize,
    locked: bool,
    focused: bool,
) {
    let title = if locked {
        " DEVICES  (locked) "
    } else {
        " DEVICES          [R]efresh "
    };

    let border_color = if focused && !locked {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    if devices.is_empty() {
        let text = vec![
            Line::from("  No devices found."),
            Line::from("  Connect device & press R"),
        ];
        let paragraph = Paragraph::new(text).block(block);
        frame.render_widget(paragraph, area);
        return;
    }

    let mut lines = Vec::new();

    // Calculate visible window
    let inner_height = block.inner(area).height as usize;
    // Reserve lines for scroll indicators
    let max_visible = if devices.len() > inner_height {
        inner_height.saturating_sub(1) // leave room for indicator
    } else {
        inner_height
    };
    let total = devices.len();

    let has_more_above = scroll_offset > 0;
    let has_more_below = scroll_offset + max_visible < total;

    if has_more_above {
        lines.push(Line::from(Span::styled(
            "  ↑ more above",
            Style::default().fg(Color::DarkGray),
        )));
    }

    let visible_end = (scroll_offset + max_visible).min(total);
    // Adjust visible count if we show both indicators
    let visible_start = scroll_offset;
    let effective_end = if has_more_above && has_more_below {
        (visible_start + max_visible.saturating_sub(1)).min(total)
    } else {
        visible_end
    };

    for (i, dev) in devices
        .iter()
        .enumerate()
        .skip(visible_start)
        .take(effective_end.saturating_sub(visible_start))
    {
        let marker = if i == selected { "> " } else { "  " };
        let mode_style = if dev.is_fel {
            Style::default().fg(Color::Red)
        } else {
            Style::default().fg(Color::Green)
        };

        // Format: > Bus 000:Port 008 FEL (0x1890)
        let line = Line::from(vec![
            Span::raw(marker),
            Span::raw(format!("Bus {:03}:Port {:03} ", dev.bus, dev.port)),
            Span::styled(&dev.mode, mode_style),
            Span::raw(format!(" (0x{:x})", dev.chip_id)),
        ]);
        lines.push(line);
    }

    if has_more_below {
        lines.push(Line::from(Span::styled(
            "  ↓ more below",
            Style::default().fg(Color::DarkGray),
        )));
    }

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}
