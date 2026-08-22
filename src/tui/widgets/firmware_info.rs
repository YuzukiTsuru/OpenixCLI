//! Firmware info and options widget

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::flash::{FlashMode, PostAction};

/// Which option row is focused inside the firmware panel
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirmwareField {
    Mode,
    Verify,
    PostAction,
    Parts,
}

impl FirmwareField {
    pub fn next(&self, has_parts: bool) -> Self {
        match self {
            FirmwareField::Mode => FirmwareField::Verify,
            FirmwareField::Verify => FirmwareField::PostAction,
            FirmwareField::PostAction => {
                if has_parts {
                    FirmwareField::Parts
                } else {
                    FirmwareField::Mode
                }
            }
            FirmwareField::Parts => FirmwareField::Mode,
        }
    }

    pub fn prev(&self, has_parts: bool) -> Self {
        match self {
            FirmwareField::Mode => {
                if has_parts {
                    FirmwareField::Parts
                } else {
                    FirmwareField::PostAction
                }
            }
            FirmwareField::Verify => FirmwareField::Mode,
            FirmwareField::PostAction => FirmwareField::Verify,
            FirmwareField::Parts => FirmwareField::PostAction,
        }
    }
}

pub struct FirmwareState {
    pub path: Option<String>,
    pub size_mb: u64,
    pub num_files: u32,
    pub mode: FlashMode,
    pub verify: bool,
    pub post_action: PostAction,
    /// All partition names from MBR (populated after firmware load)
    pub all_partitions: Vec<String>,
    /// Which partitions are selected (parallel to all_partitions)
    pub selected_partitions: Vec<bool>,
    /// Currently highlighted partition index (for Parts field)
    pub parts_cursor: usize,
    /// Scroll offset for partition list (first visible partition index)
    pub parts_scroll_offset: usize,
    /// Maximum visible partitions (calculated during render based on available height)
    pub parts_max_visible: usize,
    pub focused_field: FirmwareField,
}

impl Default for FirmwareState {
    fn default() -> Self {
        Self {
            path: None,
            size_mb: 0,
            num_files: 0,
            mode: FlashMode::FullErase,
            verify: true,
            post_action: PostAction::Reboot,
            all_partitions: Vec::new(),
            selected_partitions: Vec::new(),
            parts_cursor: 0,
            parts_scroll_offset: 0,
            parts_max_visible: 6, // Default, will be updated during render
            focused_field: FirmwareField::Mode,
        }
    }
}

impl FirmwareState {
    pub fn mode_display(&self) -> &'static str {
        self.mode.display_name()
    }

    pub fn next_mode(&mut self) {
        self.mode = self.mode.next();
    }

    pub fn prev_mode(&mut self) {
        self.mode = self.mode.prev();
    }

    /// Whether the Parts field should be navigable
    pub fn has_parts_field(&self) -> bool {
        self.mode == FlashMode::Partition && !self.all_partitions.is_empty()
    }

    /// Get the selected partition names (None = all)
    pub fn selected_partition_names(&self) -> Option<Vec<String>> {
        if self.mode != FlashMode::Partition {
            return None;
        }
        if self.all_partitions.is_empty() {
            return None;
        }
        let selected: Vec<String> = self
            .all_partitions
            .iter()
            .zip(self.selected_partitions.iter())
            .filter(|(_, &sel)| sel)
            .map(|(name, _)| name.clone())
            .collect();
        if selected.is_empty() || selected.len() == self.all_partitions.len() {
            None // all selected = same as no filter
        } else {
            Some(selected)
        }
    }

    /// Handle Left key on the currently focused field
    pub fn cycle_left(&mut self) {
        match self.focused_field {
            FirmwareField::Mode => self.prev_mode(),
            FirmwareField::Verify => self.verify = !self.verify,
            FirmwareField::PostAction => self.post_action = self.post_action.prev(),
            FirmwareField::Parts => {}
        }
    }

    /// Handle Right key on the currently focused field
    pub fn cycle_right(&mut self) {
        match self.focused_field {
            FirmwareField::Mode => self.next_mode(),
            FirmwareField::Verify => self.verify = !self.verify,
            FirmwareField::PostAction => self.post_action = self.post_action.next(),
            FirmwareField::Parts => {}
        }
    }

    /// Move partition cursor up; if at top, exit Parts field upward
    pub fn move_parts_cursor_up(&mut self) {
        if self.parts_cursor > 0 {
            self.parts_cursor -= 1;
            // Scroll up if cursor goes above visible area
            if self.parts_cursor < self.parts_scroll_offset {
                self.parts_scroll_offset = self.parts_cursor;
            }
        } else {
            // At top of list, move to previous field
            self.focused_field = FirmwareField::PostAction;
        }
    }

    /// Move partition cursor down; if at bottom, exit Parts field downward
    pub fn move_parts_cursor_down(&mut self) {
        if !self.all_partitions.is_empty() && self.parts_cursor + 1 < self.all_partitions.len() {
            self.parts_cursor += 1;
            // Scroll down if cursor goes below visible area
            let max_visible = self.parts_max_visible;
            if self.parts_cursor >= self.parts_scroll_offset + max_visible {
                self.parts_scroll_offset = self.parts_cursor + 1 - max_visible;
            }
        } else {
            // At bottom of list, wrap to first field
            self.focused_field = FirmwareField::Mode;
        }
    }

    /// Toggle the currently highlighted partition
    pub fn toggle_partition(&mut self) {
        if self.focused_field == FirmwareField::Parts
            && self.parts_cursor < self.selected_partitions.len()
        {
            self.selected_partitions[self.parts_cursor] =
                !self.selected_partitions[self.parts_cursor];
        }
    }

    /// Select/deselect all partitions
    pub fn toggle_all_partitions(&mut self) {
        let all_selected = self.selected_partitions.iter().all(|&s| s);
        self.selected_partitions.fill(!all_selected);
    }
}

fn abbreviated_path(path: &str) -> String {
    if path.chars().count() <= 35 {
        return path.to_string();
    }

    let tail: String = path
        .chars()
        .rev()
        .take(32)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("...{tail}")
}

pub fn render(
    frame: &mut Frame,
    area: Rect,
    state: &mut FirmwareState,
    locked: bool,
    focused: bool,
) {
    let title = if locked {
        " FIRMWARE  (locked) "
    } else {
        " FIRMWARE & OPTIONS "
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

    let mut lines = Vec::new();

    // Firmware path + browse hint on same line
    let path_display = match &state.path {
        Some(p) => abbreviated_path(p),
        None => "(none)".into(),
    };
    if locked {
        lines.push(Line::from(vec![
            Span::raw(" Firmware: "),
            Span::styled(&path_display, Style::default().fg(Color::White)),
        ]));
    } else {
        lines.push(Line::from(vec![
            Span::raw(" Firmware: "),
            Span::styled(&path_display, Style::default().fg(Color::White)),
            Span::raw("  "),
            Span::styled("Press [B] enter path", Style::default().fg(Color::DarkGray)),
        ]));
    }

    // Size and files
    if state.path.is_some() {
        lines.push(Line::from(format!(
            " Size: {} MB  Files: {}",
            state.size_mb, state.num_files
        )));
    }

    lines.push(Line::from(""));

    // Calculate inner width for right-aligning ">"
    // inner = area.width - 2 (borders)
    let inner_w = area.width.saturating_sub(2) as usize;
    // prefix: " > " or "   " = 3 chars
    // label: "Verify" = 6 chars + " : " = 3 chars => 12 chars before value area
    let prefix_len = 3 + 6 + 3; // 12
    let value_area = inner_w.saturating_sub(prefix_len + 1);

    // Helper to build an option row with right-aligned ">"
    // Format: {indicator}{label:6} : < {value}{padding} >
    #[allow(clippy::too_many_arguments)]
    fn build_option_line<'a>(
        indicator: &'a str,
        ind_style: Style,
        label: &'a str,
        value: &'a str,
        value_style: Style,
        arrow_style: Style,
        value_area: usize,
        has_arrows: bool,
    ) -> Line<'a> {
        let padded_label = format!("{:<6}", label);
        if has_arrows {
            // "< " = 2, " >" = 2 => content width = value_area - 4
            let content_w = value_area.saturating_sub(4);
            let padded_value = format!("{:<width$}", value, width = content_w);
            Line::from(vec![
                Span::styled(indicator, ind_style),
                Span::raw(padded_label),
                Span::raw(" : "),
                Span::styled("< ", arrow_style),
                Span::styled(padded_value, value_style),
                Span::styled(" >", arrow_style),
            ])
        } else {
            Line::from(vec![
                Span::styled(indicator, ind_style),
                Span::raw(padded_label),
                Span::raw(" : "),
                Span::raw(value.to_string()),
            ])
        }
    }

    // Mode row
    let mode_focused = focused && !locked && state.focused_field == FirmwareField::Mode;
    if locked {
        lines.push(build_option_line(
            "   ",
            Style::default(),
            "Mode",
            state.mode_display(),
            Style::default().fg(Color::Yellow),
            Style::default(),
            value_area,
            false,
        ));
    } else {
        let arrow_style = if mode_focused {
            Style::default().fg(Color::White).bold()
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let value_style = if mode_focused {
            Style::default().fg(Color::Yellow).bold()
        } else {
            Style::default().fg(Color::Yellow)
        };
        let indicator = if mode_focused { " > " } else { "   " };
        let ind_style = Style::default().fg(Color::Cyan);
        lines.push(build_option_line(
            indicator,
            ind_style,
            "Mode",
            state.mode_display(),
            value_style,
            arrow_style,
            value_area,
            true,
        ));
    }

    // Verify row
    let verify_focused = focused && !locked && state.focused_field == FirmwareField::Verify;
    let verify_str = if state.verify { "ON" } else { "OFF" };
    if locked {
        let verify_value_style = if state.verify {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::Red)
        };
        lines.push(build_option_line(
            "   ",
            Style::default(),
            "Verify",
            verify_str,
            verify_value_style,
            Style::default(),
            value_area,
            false,
        ));
    } else {
        let verify_value_style = if state.verify {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::Red)
        };
        let verify_value_style = if verify_focused {
            verify_value_style.bold()
        } else {
            verify_value_style
        };
        let arrow_style = if verify_focused {
            Style::default().fg(Color::White).bold()
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let indicator = if verify_focused { " > " } else { "   " };
        let ind_style = Style::default().fg(Color::Cyan);
        lines.push(build_option_line(
            indicator,
            ind_style,
            "Verify",
            verify_str,
            verify_value_style,
            arrow_style,
            value_area,
            true,
        ));
    }

    // Post action row
    let post_focused = focused && !locked && state.focused_field == FirmwareField::PostAction;
    if locked {
        lines.push(build_option_line(
            "   ",
            Style::default(),
            "Post",
            state.post_action.name(),
            Style::default().fg(Color::Cyan),
            Style::default(),
            value_area,
            false,
        ));
    } else {
        let arrow_style = if post_focused {
            Style::default().fg(Color::White).bold()
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let value_style = if post_focused {
            Style::default().fg(Color::Cyan).bold()
        } else {
            Style::default().fg(Color::Cyan)
        };
        let indicator = if post_focused { " > " } else { "   " };
        let ind_style = Style::default().fg(Color::Cyan);
        lines.push(build_option_line(
            indicator,
            ind_style,
            "Post",
            state.post_action.name(),
            value_style,
            arrow_style,
            value_area,
            true,
        ));
    }

    // Partitions section - each partition on its own line with scrolling
    let parts_focused = focused && !locked && state.focused_field == FirmwareField::Parts;
    let is_partition_mode = state.mode == FlashMode::Partition;

    if !state.all_partitions.is_empty() {
        // Header line
        let indicator = if parts_focused { " > " } else { "   " };
        let ind_style = Style::default().fg(Color::Cyan);
        let padded_label = format!("{:<6}", "Parts");

        if parts_focused {
            lines.push(Line::from(vec![
                Span::styled(indicator, ind_style),
                Span::raw(padded_label),
                Span::raw(" : "),
                Span::styled("Space", Style::default().fg(Color::DarkGray)),
                Span::styled(":toggle ", Style::default().fg(Color::DarkGray)),
                Span::styled("A", Style::default().fg(Color::DarkGray)),
                Span::styled(":all", Style::default().fg(Color::DarkGray)),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled(indicator, ind_style),
                Span::raw(padded_label),
                Span::raw(" : "),
            ]));
        }

        // Calculate visible partition window based on available height
        // Fixed content: firmware path (1-2 lines), size (1), blank (1), mode (1), verify (1), post (1), parts header (1)
        // = ~8 lines + borders (2) = 10 lines minimum
        let inner_height = area.height.saturating_sub(2) as usize; // Remove borders
        let fixed_lines = if state.path.is_some() { 2 } else { 1 } // path + size or just path
            + 1 // blank line
            + 1 // mode
            + 1 // verify
            + 1 // post
            + 1; // parts header

        let available_for_parts = inner_height.saturating_sub(fixed_lines);
        // Reserve 1 line for scroll indicators if needed
        let max_visible = available_for_parts.saturating_sub(2).max(3); // Minimum 3 visible

        // Store for use in cursor movement
        state.parts_max_visible = max_visible;

        let total_parts = state.all_partitions.len();
        let scroll_offset = state.parts_scroll_offset;

        let has_more_above = scroll_offset > 0;
        let has_more_below = scroll_offset.saturating_add(max_visible) < total_parts;

        // Show scroll indicator at top if needed
        if has_more_above {
            lines.push(Line::from(vec![
                Span::raw("               "),
                Span::styled("↑ more above", Style::default().fg(Color::DarkGray)),
            ]));
        }

        // Show visible partition slice
        let visible_end = scroll_offset.saturating_add(max_visible).min(total_parts);
        for i in scroll_offset..visible_end {
            let name = &state.all_partitions[i];
            let selected = state.selected_partitions.get(i).copied().unwrap_or(true);
            let is_cursor = parts_focused && i == state.parts_cursor;

            if is_partition_mode {
                // Interactive: show checkbox
                let check = if selected { "x" } else { " " };
                let chip = format!("   [{}] {}", check, name);

                let style = if is_cursor {
                    Style::default().fg(Color::White).bold().underlined()
                } else if selected {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::DarkGray)
                };

                lines.push(Line::from(vec![
                    Span::raw("            "),
                    Span::styled(chip, style),
                ]));
            } else {
                // Read-only: just list the name
                let style = if is_cursor {
                    Style::default().fg(Color::White).bold()
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                lines.push(Line::from(vec![
                    Span::raw("               "),
                    Span::styled(name.as_str(), style),
                ]));
            }
        }

        // Show scroll indicator at bottom if needed
        if has_more_below {
            lines.push(Line::from(vec![
                Span::raw("               "),
                Span::styled("↓ more below", Style::default().fg(Color::DarkGray)),
            ]));
        }
    } else if state.path.is_some() {
        // Firmware loaded but no partitions found in MBR
        lines.push(build_option_line(
            "   ",
            Style::default(),
            "Parts",
            "N/A",
            Style::default().fg(Color::DarkGray),
            Style::default(),
            value_area,
            false,
        ));
    }

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn render_state(
        state: &mut FirmwareState,
        width: u16,
        height: u16,
        locked: bool,
        focused: bool,
    ) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render(frame, frame.area(), state, locked, focused))
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
    fn field_navigation_covers_lists_with_and_without_partitions() {
        assert_eq!(FirmwareField::Mode.next(false), FirmwareField::Verify);
        assert_eq!(FirmwareField::Verify.next(false), FirmwareField::PostAction);
        assert_eq!(FirmwareField::PostAction.next(false), FirmwareField::Mode);
        assert_eq!(FirmwareField::PostAction.next(true), FirmwareField::Parts);
        assert_eq!(FirmwareField::Parts.next(true), FirmwareField::Mode);

        assert_eq!(FirmwareField::Mode.prev(false), FirmwareField::PostAction);
        assert_eq!(FirmwareField::Mode.prev(true), FirmwareField::Parts);
        assert_eq!(FirmwareField::Verify.prev(false), FirmwareField::Mode);
        assert_eq!(FirmwareField::PostAction.prev(false), FirmwareField::Verify);
        assert_eq!(FirmwareField::Parts.prev(true), FirmwareField::PostAction);
    }

    #[test]
    fn modes_options_and_partition_filters_cover_each_state() {
        let mut state = FirmwareState::default();
        assert_eq!(state.mode_display(), "Full Erase");
        assert!(!state.has_parts_field());
        assert_eq!(state.selected_partition_names(), None);

        state.next_mode();
        state.prev_mode();
        assert_eq!(state.mode, FlashMode::FullErase);

        state.focused_field = FirmwareField::Verify;
        state.cycle_left();
        assert!(!state.verify);
        state.cycle_right();
        assert!(state.verify);

        state.focused_field = FirmwareField::PostAction;
        let original_action = state.post_action;
        state.cycle_left();
        state.cycle_right();
        assert_eq!(state.post_action, original_action);

        state.focused_field = FirmwareField::Parts;
        let original_mode = state.mode;
        state.cycle_left();
        state.cycle_right();
        assert_eq!(state.mode, original_mode);

        state.mode = FlashMode::Partition;
        state.all_partitions = vec!["boot".into(), "system".into(), "vendor".into()];
        state.selected_partitions = vec![true, false, true];
        assert!(state.has_parts_field());
        assert_eq!(
            state.selected_partition_names(),
            Some(vec!["boot".to_string(), "vendor".to_string()])
        );
        state.selected_partitions.fill(true);
        assert_eq!(state.selected_partition_names(), None);
        state.selected_partitions.fill(false);
        assert_eq!(state.selected_partition_names(), None);
        state.mode = FlashMode::FullErase;
        assert_eq!(state.selected_partition_names(), None);
    }

    #[test]
    fn partition_cursor_scrolling_and_toggles_handle_boundaries() {
        let mut state = FirmwareState {
            mode: FlashMode::Partition,
            all_partitions: (0..5).map(|index| format!("part{index}")).collect(),
            selected_partitions: vec![true; 5],
            parts_max_visible: 2,
            focused_field: FirmwareField::Parts,
            ..FirmwareState::default()
        };

        state.move_parts_cursor_up();
        assert_eq!(state.focused_field, FirmwareField::PostAction);
        state.focused_field = FirmwareField::Parts;
        for _ in 0..4 {
            state.move_parts_cursor_down();
        }
        assert_eq!(state.parts_cursor, 4);
        assert_eq!(state.parts_scroll_offset, 3);
        state.move_parts_cursor_down();
        assert_eq!(state.focused_field, FirmwareField::Mode);

        state.focused_field = FirmwareField::Parts;
        state.toggle_partition();
        assert!(!state.selected_partitions[4]);
        state.toggle_all_partitions();
        assert!(state.selected_partitions.iter().all(|selected| *selected));
        state.toggle_all_partitions();
        assert!(state.selected_partitions.iter().all(|selected| !*selected));

        for _ in 0..4 {
            state.move_parts_cursor_up();
        }
        assert_eq!(state.parts_cursor, 0);
        assert_eq!(state.parts_scroll_offset, 0);

        state.parts_cursor = 99;
        let before = state.selected_partitions.clone();
        state.toggle_partition();
        assert_eq!(state.selected_partitions, before);
    }

    #[test]
    fn path_abbreviation_is_unicode_safe_and_exactly_bounded() {
        assert_eq!(abbreviated_path("short.fex"), "short.fex");
        let long = format!("{}firmware.fex", "固件/".repeat(20));
        let abbreviated = abbreviated_path(&long);
        assert!(abbreviated.starts_with("..."));
        assert_eq!(abbreviated.chars().count(), 35);
        assert!(long.ends_with(&abbreviated[3..]));
    }

    #[test]
    fn rendering_covers_empty_loaded_locked_and_scrolled_partition_views() {
        let mut empty = FirmwareState::default();
        let empty_text = render_state(&mut empty, 70, 14, false, true);
        assert!(empty_text.contains("(none)"));

        let mut loaded = FirmwareState {
            path: Some(format!("{}image.fex", "固件/".repeat(20))),
            size_mb: 128,
            num_files: 9,
            mode: FlashMode::Partition,
            all_partitions: (0..8).map(|index| format!("part{index}")).collect(),
            selected_partitions: vec![true, false, true, false, true, false, true, false],
            parts_cursor: 4,
            parts_scroll_offset: 2,
            focused_field: FirmwareField::Parts,
            ..FirmwareState::default()
        };
        let text = render_state(&mut loaded, 70, 16, false, true);
        assert!(text.contains("128 MB"));
        assert!(text.contains("[x]"));
        assert!(text.contains("[ ]"));

        let locked = render_state(&mut loaded, 70, 16, true, false);
        assert!(locked.contains("(locked)"));

        loaded.mode = FlashMode::FullErase;
        let readonly = render_state(&mut loaded, 70, 16, false, false);
        assert!(readonly.contains("part"));

        loaded.all_partitions.clear();
        loaded.selected_partitions.clear();
        let no_parts = render_state(&mut loaded, 70, 12, false, false);
        assert!(no_parts.contains("N/A"));
    }
}
