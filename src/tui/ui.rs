//! UI rendering (layout, widgets)

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

use super::app::{App, FocusPanel};
use super::widgets::{device_list, firmware_info, log_view, progress};

/// Main render function - dispatches to wide or narrow layout
pub fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();

    // Minimum size check
    if area.width < 60 || area.height < 15 {
        let msg = Paragraph::new("Terminal too small. Minimum: 60x15")
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::Red));
        let y = area.height / 2;
        frame.render_widget(
            msg,
            Rect::new(area.x, y.min(area.height.saturating_sub(1)), area.width, 1),
        );
        return;
    }

    if area.width >= 100 {
        render_wide_layout(frame, app, area);
    } else {
        render_narrow_layout(frame, app, area);
    }
}

fn render_wide_layout(frame: &mut Frame, app: &mut App, area: Rect) {
    let outer = Layout::vertical([
        Constraint::Length(1), // Title bar
        Constraint::Min(10),   // Main content
        Constraint::Length(1), // Status bar
    ])
    .split(area);

    render_title_bar(frame, outer[0]);

    let panels = Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(outer[1]);

    // Left panel
    let device_height = 4 + app.devices.len().min(5) as u16;
    let left = Layout::vertical([
        Constraint::Length(device_height.max(5)),
        Constraint::Min(10),
        Constraint::Length(3),
    ])
    .split(panels[0]);

    let dev_focused = app.focus == FocusPanel::Devices;
    let fw_focused = app.focus == FocusPanel::Options;

    let is_flashing = app.is_flashing();
    device_list::render(
        frame,
        left[0],
        &app.devices,
        app.selected_device,
        app.device_scroll_offset,
        is_flashing,
        dev_focused,
    );
    firmware_info::render(frame, left[1], &mut app.firmware, is_flashing, fw_focused);
    render_flash_button(frame, left[2], app);

    // Right panel
    let right =
        Layout::vertical([Constraint::Percentage(45), Constraint::Percentage(55)]).split(panels[1]);

    progress::render(frame, right[0], &app.progress, false);
    log_view::render(frame, right[1], &mut app.log, false);

    render_status_bar(frame, outer[2], app);
}

fn render_narrow_layout(frame: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::vertical([
        Constraint::Length(1), // Title bar
        Constraint::Length(4), // Devices
        Constraint::Min(8),    // Firmware & Options (expandable)
        Constraint::Length(3), // Flash button
        Constraint::Length(6), // Progress
        Constraint::Min(4),    // Log
        Constraint::Length(1), // Status bar
    ])
    .split(area);

    let dev_focused = app.focus == FocusPanel::Devices;
    let fw_focused = app.focus == FocusPanel::Options;

    render_title_bar(frame, chunks[0]);
    let is_flashing = app.is_flashing();
    device_list::render(
        frame,
        chunks[1],
        &app.devices,
        app.selected_device,
        app.device_scroll_offset,
        is_flashing,
        dev_focused,
    );
    firmware_info::render(frame, chunks[2], &mut app.firmware, is_flashing, fw_focused);
    render_flash_button(frame, chunks[3], app);
    progress::render(frame, chunks[4], &app.progress, false);
    log_view::render(frame, chunks[5], &mut app.log, false);
    render_status_bar(frame, chunks[6], app);
}

fn render_title_bar(frame: &mut Frame, area: Rect) {
    let bar_style = Style::default().bg(Color::Rgb(30, 30, 46)).fg(Color::Gray);

    let title_text = format!(" OpenixCLI Terminal v{}", env!("CARGO_PKG_VERSION"));
    let help_text = "[H]elp  [Q]uit";

    let min_width_for_help = title_text.len() + help_text.len() + 2;

    let title = if area.width as usize >= min_width_for_help {
        let padding_len = area.width as usize - title_text.len() - help_text.len();
        Line::from(vec![
            Span::styled(
                title_text,
                Style::default()
                    .fg(Color::Cyan)
                    .bold()
                    .bg(Color::Rgb(30, 30, 46)),
            ),
            Span::raw(" ".repeat(padding_len)),
            Span::styled(
                help_text,
                Style::default().fg(Color::Gray).bg(Color::Rgb(30, 30, 46)),
            ),
        ])
    } else {
        Line::from(vec![Span::styled(
            title_text,
            Style::default()
                .fg(Color::Cyan)
                .bold()
                .bg(Color::Rgb(30, 30, 46)),
        )])
    };

    frame.render_widget(Paragraph::new(title).style(bar_style), area);
}

fn render_flash_button(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let (text, style) = if app.is_flashing() {
        ("  Flashing...  ", Style::default().fg(Color::Yellow).bold())
    } else if app.can_flash() {
        (
            ">>> [ FLASH ] <<<  Press Enter",
            Style::default().fg(Color::Green).bold(),
        )
    } else {
        (
            "  [ FLASH ] (disabled)  ",
            Style::default().fg(Color::DarkGray),
        )
    };

    let paragraph = Paragraph::new(Span::styled(text, style))
        .block(block)
        .alignment(Alignment::Center);
    frame.render_widget(paragraph, area);
}

fn render_status_bar(frame: &mut Frame, area: Rect, app: &App) {
    let bar_style = Style::default().bg(Color::Rgb(30, 30, 46)).fg(Color::Gray);

    let line = if app.is_flashing() {
        Line::from(vec![Span::styled(
            " Flashing in progress...  Ctrl+C to abort",
            Style::default()
                .fg(Color::Yellow)
                .bg(Color::Rgb(30, 30, 46)),
        )])
    } else if app.input_mode {
        let input_text = format!(
            " Path: [{}] | Enter: confirm  Esc: cancel",
            app.input_buffer
        );
        Line::from(Span::styled(
            input_text,
            Style::default().fg(Color::White).bg(Color::Rgb(30, 30, 46)),
        ))
    } else {
        Line::from(vec![
            Span::styled(
                " Tab",
                Style::default().fg(Color::Cyan).bg(Color::Rgb(30, 30, 46)),
            ),
            Span::raw(": panel  "),
            Span::styled(
                "\u{2190}\u{2192}",
                Style::default().fg(Color::Cyan).bg(Color::Rgb(30, 30, 46)),
            ),
            Span::raw(": change  "),
            Span::styled(
                "\u{2191}\u{2193}",
                Style::default().fg(Color::Cyan).bg(Color::Rgb(30, 30, 46)),
            ),
            Span::raw(": select  "),
            Span::styled(
                "R",
                Style::default().fg(Color::Cyan).bg(Color::Rgb(30, 30, 46)),
            ),
            Span::raw(": scan  "),
            Span::styled(
                "B",
                Style::default().fg(Color::Cyan).bg(Color::Rgb(30, 30, 46)),
            ),
            Span::raw(": firmware  "),
            if app.can_flash() {
                Span::styled(
                    "Enter: flash",
                    Style::default().fg(Color::Green).bg(Color::Rgb(30, 30, 46)),
                )
            } else {
                Span::raw("")
            },
        ])
    };

    frame.render_widget(Paragraph::new(line).style(bar_style), area);
}

/// Render help overlay
pub fn render_help_overlay(frame: &mut Frame) {
    let area = frame.area();
    let w = 50u16.min(area.width.saturating_sub(4));
    let h = 20u16.min(area.height.saturating_sub(4));
    let x = (area.width - w) / 2;
    let y = (area.height - h) / 2;
    let popup_area = Rect::new(x, y, w, h);

    frame.render_widget(ratatui::widgets::Clear, popup_area);

    let block = Block::default()
        .title(" Help ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));

    let help_text = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  Q / Esc     ", Style::default().fg(Color::Cyan)),
            Span::raw("Quit application"),
        ]),
        Line::from(vec![
            Span::styled("  Tab         ", Style::default().fg(Color::Cyan)),
            Span::raw("Switch focus: Devices / Options"),
        ]),
        Line::from(vec![
            Span::styled("  Left/Right  ", Style::default().fg(Color::Cyan)),
            Span::raw("Cycle option values"),
        ]),
        Line::from(vec![
            Span::styled("  Up/Down     ", Style::default().fg(Color::Cyan)),
            Span::raw("Select device / option / partition"),
        ]),
        Line::from(vec![
            Span::styled("  R           ", Style::default().fg(Color::Cyan)),
            Span::raw("Refresh device scan"),
        ]),
        Line::from(vec![
            Span::styled("  B           ", Style::default().fg(Color::Cyan)),
            Span::raw("Browse / enter firmware path"),
        ]),
        Line::from(vec![
            Span::styled("  Enter       ", Style::default().fg(Color::Cyan)),
            Span::raw("Start flash"),
        ]),
        Line::from(vec![
            Span::styled("  M           ", Style::default().fg(Color::Cyan)),
            Span::raw("Cycle flash mode"),
        ]),
        Line::from(vec![
            Span::styled("  V           ", Style::default().fg(Color::Cyan)),
            Span::raw("Toggle verify"),
        ]),
        Line::from(vec![
            Span::styled("  A           ", Style::default().fg(Color::Cyan)),
            Span::raw("Cycle post action / Select all parts"),
        ]),
        Line::from(vec![
            Span::styled("  Space       ", Style::default().fg(Color::Cyan)),
            Span::raw("Toggle partition (Partition mode)"),
        ]),
        Line::from(vec![
            Span::styled("  H           ", Style::default().fg(Color::Cyan)),
            Span::raw("Toggle this help"),
        ]),
        Line::from(vec![
            Span::styled("  Ctrl+C      ", Style::default().fg(Color::Cyan)),
            Span::raw("Abort / Quit"),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  Press any key to close",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let paragraph = Paragraph::new(help_text).block(block);
    frame.render_widget(paragraph, popup_area);
}
