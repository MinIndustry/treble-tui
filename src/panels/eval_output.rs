use ratatui::{
    buffer::Buffer as RBuf,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget, Wrap},
};

/// A single eval output entry.
#[derive(Debug, Clone)]
pub struct EvalEntry {
    pub timestamp: String,
    pub kind: EvalEntryKind,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvalEntryKind {
    Info,
    Success,
    Warning,
    Error,
}

impl EvalEntryKind {
    fn color(&self) -> Color {
        match self {
            EvalEntryKind::Info => Color::Cyan,
            EvalEntryKind::Success => Color::Green,
            EvalEntryKind::Warning => Color::Yellow,
            EvalEntryKind::Error => Color::Red,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            EvalEntryKind::Info => "INFO",
            EvalEntryKind::Success => " OK ",
            EvalEntryKind::Warning => "WARN",
            EvalEntryKind::Error => " ERR",
        }
    }
}

/// The eval output / log panel (Column 2).
/// Displays compilation results, errors, and live feedback.
pub struct EvalOutputPanel<'a> {
    entries: &'a [EvalEntry],
    scroll: usize,
    focused: bool,
}

impl<'a> EvalOutputPanel<'a> {
    pub fn new(entries: &'a [EvalEntry], scroll: usize, focused: bool) -> Self {
        Self {
            entries,
            scroll,
            focused,
        }
    }
}

impl Widget for EvalOutputPanel<'_> {
    fn render(self, area: Rect, buf: &mut RBuf) {
        let border_style = if self.focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let title = format!(" Eval Output ({}) ", self.entries.len());
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(title);

        let inner = block.inner(area);
        block.render(area, buf);

        if inner.width == 0 || inner.height == 0 {
            return;
        }

        if self.entries.is_empty() {
            let placeholder = Paragraph::new(Line::from(vec![Span::styled(
                "No output yet. Save (:w / Ctrl+S) to evaluate.",
                Style::default().fg(Color::DarkGray),
            )]));
            placeholder.render(inner, buf);
            return;
        }

        let viewport_height = inner.height as usize;
        let visible_entries = self
            .entries
            .iter()
            .skip(self.scroll)
            .take(viewport_height);

        let lines: Vec<Line> = visible_entries
            .map(|entry| {
                Line::from(vec![
                    Span::styled(
                        format!("[{}]", entry.timestamp),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(
                        format!(" [{}] ", entry.kind.label()),
                        Style::default()
                            .fg(entry.kind.color())
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(entry.message.clone(), Style::default().fg(Color::White)),
                ])
            })
            .collect();

        let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
        paragraph.render(inner, buf);
    }
}
