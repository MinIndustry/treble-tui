use ratatui::{
    buffer::Buffer as RBuf,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget, Wrap},
};

/// Context information displayed in the reference panel.
#[derive(Debug, Clone)]
pub struct ContextInfo {
    /// Available instruments and their status.
    pub instruments: Vec<InstrumentInfo>,
    /// Available keybinding hints for the current mode.
    pub keybindings: Vec<(String, String)>,
    /// Current playback / engine state summary.
    pub engine_status: String,
}

#[derive(Debug, Clone)]
pub struct InstrumentInfo {
    pub name: String,
    pub active: bool,
    pub voice_count: usize,
}

impl Default for ContextInfo {
    fn default() -> Self {
        Self {
            instruments: Vec::new(),
            keybindings: Vec::new(),
            engine_status: "Engine: idle".to_string(),
        }
    }
}

/// The context / reference panel (Column 3).
/// Shows instrument state, keybinding hints, and engine status.
pub struct ContextPanel<'a> {
    info: &'a ContextInfo,
    focused: bool,
}

impl<'a> ContextPanel<'a> {
    pub fn new(info: &'a ContextInfo, focused: bool) -> Self {
        Self { info, focused }
    }
}

impl Widget for ContextPanel<'_> {
    fn render(self, area: Rect, buf: &mut RBuf) {
        let border_style = if self.focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(" Context ");

        let inner = block.inner(area);
        block.render(area, buf);

        if inner.width == 0 || inner.height == 0 {
            return;
        }

        let mut lines: Vec<Line> = Vec::new();

        // Engine status
        lines.push(Line::from(vec![
            Span::styled(
                "Engine Status",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            ),
        ]));
        lines.push(Line::from(vec![Span::styled(
            format!("  {}", self.info.engine_status),
            Style::default().fg(Color::White),
        )]));
        lines.push(Line::from(""));

        // Instruments section
        lines.push(Line::from(vec![
            Span::styled(
                "Instruments",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            ),
        ]));

        if self.info.instruments.is_empty() {
            lines.push(Line::from(vec![Span::styled(
                "  (none loaded)",
                Style::default().fg(Color::DarkGray),
            )]));
        } else {
            for inst in &self.info.instruments {
                let status_color = if inst.active {
                    Color::Green
                } else {
                    Color::DarkGray
                };
                let indicator = if inst.active { "+" } else { "-" };
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("  {} ", indicator),
                        Style::default().fg(status_color),
                    ),
                    Span::styled(
                        inst.name.clone(),
                        Style::default().fg(Color::White),
                    ),
                    Span::styled(
                        format!(" ({} voices)", inst.voice_count),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));
            }
        }
        lines.push(Line::from(""));

        // Keybinding hints
        lines.push(Line::from(vec![
            Span::styled(
                "Keybindings",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            ),
        ]));

        for (key, desc) in &self.info.keybindings {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {:>12}", key),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("  {}", desc),
                    Style::default().fg(Color::White),
                ),
            ]));
        }

        let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
        paragraph.render(inner, buf);
    }
}
