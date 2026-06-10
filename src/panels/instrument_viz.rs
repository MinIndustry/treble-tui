use ratatui::{
    buffer::Buffer as RBuf,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

/// Display-ready pattern row for the sequence visualizer.
#[derive(Debug, Clone)]
pub struct SequencePatternView {
    pub pattern_name: String,
    pub instrument: String,
    pub phase: f64,
    pub current_step: usize,
    pub steps: Vec<(String, f64, f64)>,
}

/// Sequence timeline visualizer with a moving playhead.
pub struct SequenceVizPanel<'a> {
    patterns: &'a [SequencePatternView],
    global_cycle: u64,
    cycle_phase: f64,
    focused: bool,
}

impl<'a> SequenceVizPanel<'a> {
    pub fn new(
        patterns: &'a [SequencePatternView],
        global_cycle: u64,
        cycle_phase: f64,
        focused: bool,
    ) -> Self {
        Self {
            patterns,
            global_cycle,
            cycle_phase,
            focused,
        }
    }

    pub const CHROME: u16 = 2;
    pub const LINES_PER_PATTERN: u16 = 2;
    pub const PREFIX_WIDTH: u16 = 14;

    pub fn height_for_patterns(n: usize) -> u16 {
        Self::CHROME + (n.max(1) as u16) * Self::LINES_PER_PATTERN
    }
}

impl Widget for SequenceVizPanel<'_> {
    fn render(self, area: Rect, buf: &mut RBuf) {
        let border_style = if self.focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let title = if self.patterns.is_empty() {
            " Sequences ".to_string()
        } else {
            format!(
                " Sequences  cycle {}  beat {:>3.0}% ",
                self.global_cycle,
                self.cycle_phase * 100.0
            )
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(title);

        let inner = block.inner(area);
        block.render(area, buf);

        if inner.width == 0 || inner.height == 0 {
            return;
        }

        if self.patterns.is_empty() {
            Paragraph::new(Line::from(Span::styled(
                "Evaluate (:w) to start sequences",
                Style::default().fg(Color::DarkGray),
            )))
            .render(inner, buf);
            return;
        }

        let max_patterns = (inner.height / Self::LINES_PER_PATTERN) as usize;
        let timeline_w = inner.width.saturating_sub(SequenceVizPanel::PREFIX_WIDTH) as usize;

        for (i, pattern) in self.patterns.iter().take(max_patterns).enumerate() {
            let row = inner.y + (i as u16 * Self::LINES_PER_PATTERN);
            if row + 1 >= inner.y + inner.height {
                break;
            }

            render_pattern(
                buf,
                inner.x,
                row,
                inner.width,
                pattern.pattern_name.as_str(),
                pattern.instrument.as_str(),
                pattern.phase,
                pattern.current_step,
                &pattern.steps,
                timeline_w,
            );
        }
    }
}

fn render_pattern(
    buf: &mut RBuf,
    x: u16,
    y: u16,
    total_width: u16,
    pattern_name: &str,
    instrument: &str,
    phase: f64,
    current_step: usize,
    steps: &[(String, f64, f64)],
    timeline_w: usize,
) {
    let prefix = format!(
        "{:>5}→{:<6}",
        trunc(pattern_name, 5),
        trunc(instrument, 6)
    );
    Paragraph::new(Line::from(Span::styled(
        prefix,
        Style::default().fg(Color::White),
    )))
    .render(
        Rect {
            x,
            y,
            width: SequenceVizPanel::PREFIX_WIDTH.min(total_width),
            height: 1,
        },
        buf,
    );

    if timeline_w == 0 {
        return;
    }

    let (seg_at_col, chars) = layout_timeline(steps, timeline_w);
    let playhead = ((phase * timeline_w as f64) as usize).min(timeline_w.saturating_sub(1));
    let timeline_x = x + SequenceVizPanel::PREFIX_WIDTH;

    for (col, ch) in chars.iter().enumerate() {
        let seg_idx = seg_at_col[col];
        let px = timeline_x + col as u16;
        if px >= x + total_width {
            break;
        }

        let style = if col == playhead {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else if seg_idx == current_step {
            Style::default().fg(Color::Green)
        } else if *ch == 'x' {
            Style::default().fg(Color::Cyan)
        } else if *ch == '~' {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default().fg(Color::White)
        };

        let sym = ch.to_string();
        buf[(px, y)].set_symbol(sym.as_str()).set_style(style);
    }

    let caret_x = timeline_x + playhead as u16;
    if caret_x < x + total_width {
        buf[(caret_x, y + 1)]
            .set_symbol("^")
            .set_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
    }
}

fn layout_timeline(steps: &[(String, f64, f64)], width: usize) -> (Vec<usize>, Vec<char>) {
    let mut seg_at = vec![0usize; width];
    let mut chars = vec!['·'; width];

    if steps.is_empty() {
        return (seg_at, chars);
    }

    let mut col = 0usize;
    for (i, (label, start, end)) in steps.iter().enumerate() {
        let frac = (end - start).max(0.0);
        let w = ((frac * width as f64).round() as usize).max(1);
        let label_chars: Vec<char> = label.chars().collect();
        let fill = label_chars.first().copied().unwrap_or('·');

        for j in 0..w {
            if col >= width {
                break;
            }
            seg_at[col] = i;
            chars[col] = label_chars.get(j).copied().unwrap_or(fill);
            col += 1;
        }
    }

    let last = steps.len().saturating_sub(1);
    while col < width {
        seg_at[col] = last;
        chars[col] = '~';
        col += 1;
    }

    (seg_at, chars)
}

fn trunc(s: &str, max: usize) -> String {
    if s.len() <= max {
        format!("{s:width$}", width = max)
    } else {
        format!("{}…", &s[..max.saturating_sub(1)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_proportional_widths() {
        let steps = vec![
            ("x".into(), 0.0, 0.5),
            ("~".into(), 0.5, 1.0),
        ];
        let (seg, chars) = layout_timeline(&steps, 8);
        assert_eq!(seg[0], 0);
        assert_eq!(seg[7], 1);
        assert_eq!(chars[0], 'x');
        assert_eq!(chars[7], '~');
    }
}
