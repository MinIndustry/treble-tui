use ratatui::{
    buffer::Buffer as RBuf,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

use crate::editor::{Buffer, Mode};

/// The main code editor panel (Column 1).
/// Displays the Buffer contents with line numbers, cursor, and visual selection.
pub struct CodeEditorPanel<'a> {
    buffer: &'a Buffer,
    mode: &'a Mode,
    focused: bool,
}

impl<'a> CodeEditorPanel<'a> {
    pub fn new(buffer: &'a Buffer, mode: &'a Mode, focused: bool) -> Self {
        Self {
            buffer,
            mode,
            focused,
        }
    }
}

impl Widget for CodeEditorPanel<'_> {
    fn render(self, area: Rect, buf: &mut RBuf) {
        let border_style = if self.focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let title = format!(" {} {} ", self.buffer.name, if self.buffer.dirty { "[+]" } else { "" });
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(title);

        let inner = block.inner(area);
        block.render(area, buf);

        if inner.width == 0 || inner.height == 0 {
            return;
        }

        let line_num_width = format!("{}", self.buffer.line_count()).len().max(3);
        let viewport_height = inner.height as usize;
        let text_width = (inner.width as usize).saturating_sub(line_num_width + 2);

        let visual_range = self.buffer.visual_range();

        let mut lines: Vec<Line> = Vec::with_capacity(viewport_height);
        for vy in 0..viewport_height {
            let line_idx = self.buffer.scroll_y + vy;
            if line_idx >= self.buffer.line_count() {
                // Tilde for empty lines (like vim)
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("{:>width$} ", "~", width = line_num_width),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));
                continue;
            }

            let line_text = self.buffer.line(line_idx);
            let display_text: String = if line_text.len() > self.buffer.scroll_x {
                line_text[self.buffer.scroll_x..]
                    .chars()
                    .take(text_width)
                    .collect()
            } else {
                String::new()
            };

            // Line number
            let line_num_style = if line_idx == self.buffer.cursor_row {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            let line_num_str = format!("{:>width$} ", line_idx + 1, width = line_num_width);

            // Build spans for the line content, handling cursor and visual selection
            let content_spans = self.build_line_spans(
                &display_text,
                line_idx,
                visual_range,
                text_width,
            );

            let mut spans = vec![Span::styled(line_num_str, line_num_style)];
            spans.extend(content_spans);
            lines.push(Line::from(spans));
        }

        let paragraph = Paragraph::new(lines);
        paragraph.render(inner, buf);
    }
}

impl CodeEditorPanel<'_> {
    fn build_line_spans(
        &self,
        display_text: &str,
        line_idx: usize,
        visual_range: Option<(usize, usize, usize, usize)>,
        text_width: usize,
    ) -> Vec<Span<'static>> {
        let is_cursor_line = line_idx == self.buffer.cursor_row;
        let cursor_col = if is_cursor_line && self.focused {
            Some(self.buffer.cursor_col.saturating_sub(self.buffer.scroll_x))
        } else {
            None
        };

        // Pad display text to fill width for cursor visibility at EOL
        let padded: String = if display_text.len() < text_width {
            format!("{:<width$}", display_text, width = text_width)
        } else {
            display_text.to_string()
        };

        let chars: Vec<char> = padded.chars().collect();

        // Visual selection range for this line (in display coordinates)
        let vis_range = visual_range.and_then(|(sr, sc, er, ec)| {
            if line_idx < sr || line_idx > er {
                return None;
            }
            let start = if line_idx == sr {
                sc.saturating_sub(self.buffer.scroll_x)
            } else {
                0
            };
            let end = if line_idx == er {
                ec.saturating_sub(self.buffer.scroll_x)
            } else {
                chars.len()
            };
            Some((start, end))
        });

        let mut spans = Vec::new();
        let mut i = 0;
        while i < chars.len() {
            let in_visual = vis_range
                .map(|(s, e)| i >= s && i < e)
                .unwrap_or(false);
            let is_cursor = cursor_col.map(|c| i == c).unwrap_or(false);

            let style = if is_cursor && *self.mode != Mode::Insert {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::White)
            } else if is_cursor && *self.mode == Mode::Insert {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Green)
            } else if in_visual {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Blue)
            } else {
                Style::default().fg(Color::White)
            };

            // Batch consecutive characters with the same style
            let mut j = i + 1;
            while j < chars.len() {
                let next_in_visual = vis_range
                    .map(|(s, e)| j >= s && j < e)
                    .unwrap_or(false);
                let next_is_cursor = cursor_col.map(|c| j == c).unwrap_or(false);
                // A cursor position always breaks the batch
                if next_is_cursor || (is_cursor && j == i + 1) {
                    break;
                }
                if next_in_visual != in_visual {
                    break;
                }
                j += 1;
            }

            let text: String = chars[i..j].iter().collect();
            spans.push(Span::styled(text, style));
            i = j;
        }

        if spans.is_empty() {
            // Empty line with cursor
            if let Some(0) = cursor_col {
                spans.push(Span::styled(
                    " ".to_string(),
                    Style::default().fg(Color::Black).bg(Color::White),
                ));
            }
        }

        spans
    }
}
