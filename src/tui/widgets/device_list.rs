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
    let has_more_below = scroll_offset.saturating_add(max_visible) < total;

    if has_more_above {
        lines.push(Line::from(Span::styled(
            "  ↑ more above",
            Style::default().fg(Color::DarkGray),
        )));
    }

    let visible_end = scroll_offset.saturating_add(max_visible).min(total);
    // Adjust visible count if we show both indicators
    let visible_start = scroll_offset;
    let effective_end = if has_more_above && has_more_below {
        visible_start
            .saturating_add(max_visible.saturating_sub(1))
            .min(total)
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

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn device(index: u8, is_fel: bool) -> DeviceInfo {
        DeviceInfo {
            bus: index,
            port: index + 1,
            mode: if is_fel { "FEL" } else { "FES" }.into(),
            chip: "chip".into(),
            chip_id: 0x1800 + u32::from(index),
            is_fel,
        }
    }

    fn render_devices(
        devices: &[DeviceInfo],
        selected: usize,
        scroll_offset: usize,
        locked: bool,
        focused: bool,
        height: u16,
    ) -> String {
        let backend = TestBackend::new(50, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render(
                    frame,
                    frame.area(),
                    devices,
                    selected,
                    scroll_offset,
                    locked,
                    focused,
                )
            })
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
    fn renders_empty_selected_locked_and_scrolled_device_lists() {
        let empty = render_devices(&[], 0, 0, false, true, 5);
        assert!(empty.contains("No devices found"));

        let devices: Vec<_> = (0..8).map(|index| device(index, index % 2 == 0)).collect();
        let first = render_devices(&devices, 0, 0, false, true, 5);
        assert!(first.contains("> Bus 000:Port 001 FEL"));
        assert!(first.contains("more below"));

        let middle = render_devices(&devices, 4, 3, false, false, 6);
        assert!(middle.contains("more above"));
        assert!(middle.contains("more below"));

        let locked = render_devices(&devices[..1], 0, 0, true, true, 4);
        assert!(locked.contains("(locked)"));
        assert!(locked.contains("FEL"));

        let out_of_range = render_devices(&devices, 99, usize::MAX, false, false, 3);
        assert!(out_of_range.contains("DEVICES"));
    }
}
