use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// Manages a three-column resizable layout.
///
/// Column widths are stored as integer ratios that always sum to 100.
/// Keybinds allow resizing by shifting weight between adjacent columns.
pub struct ColumnLayout {
    /// Width ratios for each column, summing to 100.
    ratios: [u16; 3],
    /// Which column is currently focused (0, 1, or 2).
    pub focused: usize,
    /// Minimum ratio any column can shrink to.
    min_ratio: u16,
    /// Step size for resize operations.
    resize_step: u16,
}

impl ColumnLayout {
    pub fn new() -> Self {
        Self {
            ratios: [40, 30, 30],
            focused: 0,
            min_ratio: 10,
            resize_step: 5,
        }
    }

    /// Split the given area into three column rects.
    pub fn split(&self, area: Rect) -> [Rect; 3] {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(self.ratios[0]),
                Constraint::Percentage(self.ratios[1]),
                Constraint::Percentage(self.ratios[2]),
            ])
            .split(area);
        [chunks[0], chunks[1], chunks[2]]
    }

    /// Focus the next column (wrapping).
    pub fn focus_next(&mut self) {
        self.focused = (self.focused + 1) % 3;
    }

    /// Focus the previous column (wrapping).
    pub fn focus_prev(&mut self) {
        self.focused = (self.focused + 2) % 3;
    }

    /// Focus a specific column by index.
    pub fn focus(&mut self, idx: usize) {
        if idx < 3 {
            self.focused = idx;
        }
    }

    /// Grow the focused column by stealing from the next one.
    pub fn grow_focused(&mut self) {
        let next = (self.focused + 1) % 3;
        if self.ratios[next] > self.min_ratio {
            let step = self.resize_step.min(self.ratios[next] - self.min_ratio);
            self.ratios[self.focused] += step;
            self.ratios[next] -= step;
        }
    }

    /// Shrink the focused column by giving to the next one.
    pub fn shrink_focused(&mut self) {
        let next = (self.focused + 1) % 3;
        if self.ratios[self.focused] > self.min_ratio {
            let step = self
                .resize_step
                .min(self.ratios[self.focused] - self.min_ratio);
            self.ratios[self.focused] -= step;
            self.ratios[next] += step;
        }
    }

    /// Reset ratios to default (40-30-30).
    pub fn reset(&mut self) {
        self.ratios = [40, 30, 30];
    }

    /// Get the current ratios (for display in status bar).
    pub fn ratios(&self) -> [u16; 3] {
        self.ratios
    }

    pub fn focused_index(&self) -> usize {
        self.focused
    }
}
