/// A text buffer with cursor tracking and basic editing operations.
/// Each panel that needs text editing gets its own Buffer instance.
#[derive(Debug, Clone)]
pub struct Buffer {
    /// Lines of text (each line excludes the trailing newline)
    lines: Vec<String>,
    /// Cursor row (0-indexed line number)
    pub cursor_row: usize,
    /// Cursor column (0-indexed byte offset within the line)
    pub cursor_col: usize,
    /// Vertical scroll offset (first visible line)
    pub scroll_y: usize,
    /// Horizontal scroll offset
    pub scroll_x: usize,
    /// Visual mode anchor position (row, col)
    pub visual_anchor: Option<(usize, usize)>,
    /// Whether the buffer has been modified since last save/eval
    pub dirty: bool,
    /// Buffer name / title
    pub name: String,
}

impl Buffer {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            lines: vec![String::new()],
            cursor_row: 0,
            cursor_col: 0,
            scroll_y: 0,
            scroll_x: 0,
            visual_anchor: None,
            dirty: false,
            name: name.into(),
        }
    }

    pub fn from_text(name: impl Into<String>, text: &str) -> Self {
        let lines: Vec<String> = if text.is_empty() {
            vec![String::new()]
        } else {
            text.lines().map(String::from).collect()
        };
        Self {
            lines,
            cursor_row: 0,
            cursor_col: 0,
            scroll_y: 0,
            scroll_x: 0,
            visual_anchor: None,
            dirty: false,
            name: name.into(),
        }
    }

    // --- Accessors ---

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn line(&self, idx: usize) -> &str {
        self.lines.get(idx).map(|s| s.as_str()).unwrap_or("")
    }

    pub fn current_line(&self) -> &str {
        self.line(self.cursor_row)
    }

    pub fn current_line_len(&self) -> usize {
        self.current_line().len()
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub fn content(&self) -> String {
        self.lines.join("\n")
    }

    // --- Cursor movement ---

    pub fn move_up(&mut self, n: usize) {
        self.cursor_row = self.cursor_row.saturating_sub(n);
        self.clamp_cursor_col();
    }

    pub fn move_down(&mut self, n: usize) {
        self.cursor_row = (self.cursor_row + n).min(self.lines.len().saturating_sub(1));
        self.clamp_cursor_col();
    }

    pub fn move_left(&mut self, n: usize) {
        self.cursor_col = self.cursor_col.saturating_sub(n);
    }

    pub fn move_right(&mut self, n: usize) {
        let max_col = self.current_line_len();
        self.cursor_col = (self.cursor_col + n).min(max_col);
    }

    pub fn move_to_line_start(&mut self) {
        self.cursor_col = 0;
    }

    pub fn move_to_line_end(&mut self) {
        self.cursor_col = self.current_line_len();
    }

    pub fn move_to_first_non_blank(&mut self) {
        let line = self.current_line();
        self.cursor_col = line
            .chars()
            .position(|c| !c.is_whitespace())
            .unwrap_or(0);
    }

    pub fn move_to_top(&mut self) {
        self.cursor_row = 0;
        self.clamp_cursor_col();
    }

    pub fn move_to_bottom(&mut self) {
        self.cursor_row = self.lines.len().saturating_sub(1);
        self.clamp_cursor_col();
    }

    /// Move forward to the start of the next word.
    pub fn move_word_forward(&mut self) {
        let line = self.current_line().to_string();
        let len = line.len();

        if self.cursor_col >= len {
            // Move to next line
            if self.cursor_row < self.lines.len() - 1 {
                self.cursor_row += 1;
                self.cursor_col = 0;
                self.move_to_first_non_blank();
            }
            return;
        }

        let bytes = line.as_bytes();
        let mut col = self.cursor_col;

        // Skip current word characters
        while col < len && !bytes[col].is_ascii_whitespace() {
            col += 1;
        }
        // Skip whitespace
        while col < len && bytes[col].is_ascii_whitespace() {
            col += 1;
        }

        if col >= len && self.cursor_row < self.lines.len() - 1 {
            self.cursor_row += 1;
            self.cursor_col = 0;
            self.move_to_first_non_blank();
        } else {
            self.cursor_col = col.min(len);
        }
    }

    /// Move backward to the start of the previous word.
    pub fn move_word_backward(&mut self) {
        if self.cursor_col == 0 {
            if self.cursor_row > 0 {
                self.cursor_row -= 1;
                self.cursor_col = self.current_line_len();
            }
            return;
        }

        let line = self.current_line().to_string();
        let bytes = line.as_bytes();
        let mut col = self.cursor_col.saturating_sub(1);

        // Skip whitespace backward
        while col > 0 && bytes[col].is_ascii_whitespace() {
            col -= 1;
        }
        // Skip word characters backward
        while col > 0 && !bytes[col - 1].is_ascii_whitespace() {
            col -= 1;
        }

        self.cursor_col = col;
    }

    /// Move to end of current word.
    pub fn move_word_end(&mut self) {
        let line = self.current_line().to_string();
        let len = line.len();

        if self.cursor_col >= len.saturating_sub(1) {
            if self.cursor_row < self.lines.len() - 1 {
                self.cursor_row += 1;
                self.cursor_col = 0;
            }
            return;
        }

        let bytes = line.as_bytes();
        let mut col = self.cursor_col + 1;

        // Skip whitespace
        while col < len && bytes[col].is_ascii_whitespace() {
            col += 1;
        }
        // Skip word characters
        while col < len && !bytes[col].is_ascii_whitespace() {
            col += 1;
        }

        self.cursor_col = (col.saturating_sub(1)).min(len.saturating_sub(1));
    }

    // --- Editing operations (insert mode) ---

    pub fn insert_char(&mut self, ch: char) {
        let col = self.cursor_col.min(self.lines[self.cursor_row].len());
        self.lines[self.cursor_row].insert(col, ch);
        self.cursor_col = col + ch.len_utf8();
        self.dirty = true;
    }

    pub fn insert_newline(&mut self) {
        let col = self.cursor_col.min(self.lines[self.cursor_row].len());
        let rest = self.lines[self.cursor_row][col..].to_string();
        self.lines[self.cursor_row].truncate(col);
        self.cursor_row += 1;
        self.lines.insert(self.cursor_row, rest);
        self.cursor_col = 0;
        self.dirty = true;
    }

    pub fn backspace(&mut self) {
        if self.cursor_col > 0 {
            let col = self.cursor_col.min(self.lines[self.cursor_row].len());
            if col > 0 {
                self.lines[self.cursor_row].remove(col - 1);
                self.cursor_col = col - 1;
                self.dirty = true;
            }
        } else if self.cursor_row > 0 {
            // Join with previous line
            let current = self.lines.remove(self.cursor_row);
            self.cursor_row -= 1;
            self.cursor_col = self.lines[self.cursor_row].len();
            self.lines[self.cursor_row].push_str(&current);
            self.dirty = true;
        }
    }

    pub fn delete_char(&mut self) {
        let line_len = self.lines[self.cursor_row].len();
        if self.cursor_col < line_len {
            self.lines[self.cursor_row].remove(self.cursor_col);
            self.dirty = true;
        } else if self.cursor_row < self.lines.len() - 1 {
            // Join with next line
            let next = self.lines.remove(self.cursor_row + 1);
            self.lines[self.cursor_row].push_str(&next);
            self.dirty = true;
        }
    }

    /// Open a new line below the cursor and position cursor there.
    pub fn open_line_below(&mut self) {
        self.cursor_row += 1;
        self.lines.insert(self.cursor_row, String::new());
        self.cursor_col = 0;
        self.dirty = true;
    }

    /// Open a new line above the cursor and position cursor there.
    pub fn open_line_above(&mut self) {
        self.lines.insert(self.cursor_row, String::new());
        self.cursor_col = 0;
        self.dirty = true;
    }

    /// Delete the entire current line.
    pub fn delete_line(&mut self) {
        if self.lines.len() > 1 {
            self.lines.remove(self.cursor_row);
            if self.cursor_row >= self.lines.len() {
                self.cursor_row = self.lines.len() - 1;
            }
        } else {
            self.lines[0].clear();
            self.cursor_col = 0;
        }
        self.dirty = true;
    }

    /// Delete from cursor to end of line.
    pub fn delete_to_end_of_line(&mut self) {
        let col = self.cursor_col.min(self.lines[self.cursor_row].len());
        self.lines[self.cursor_row].truncate(col);
        self.clamp_cursor_col();
        self.dirty = true;
    }

    // --- Visual mode ---

    pub fn start_visual(&mut self) {
        self.visual_anchor = Some((self.cursor_row, self.cursor_col));
    }

    pub fn end_visual(&mut self) {
        self.visual_anchor = None;
    }

    /// Get the visual selection range as (start_row, start_col, end_row, end_col).
    pub fn visual_range(&self) -> Option<(usize, usize, usize, usize)> {
        let (ar, ac) = self.visual_anchor?;
        let (cr, cc) = (self.cursor_row, self.cursor_col);
        if (ar, ac) <= (cr, cc) {
            Some((ar, ac, cr, cc))
        } else {
            Some((cr, cc, ar, ac))
        }
    }

    /// Delete the visual selection.
    pub fn delete_visual_selection(&mut self) {
        if let Some((sr, sc, er, ec)) = self.visual_range() {
            if sr == er {
                // Single line
                let end = ec.min(self.lines[sr].len());
                let start = sc.min(end);
                self.lines[sr].drain(start..end);
            } else {
                // Multi-line: keep start of first line + end of last line
                let last_line_remainder = if ec < self.lines[er].len() {
                    self.lines[er][ec..].to_string()
                } else {
                    String::new()
                };
                self.lines[sr].truncate(sc);
                self.lines[sr].push_str(&last_line_remainder);
                // Remove lines between sr+1..=er
                for _ in (sr + 1)..=er {
                    if sr + 1 < self.lines.len() {
                        self.lines.remove(sr + 1);
                    }
                }
            }
            self.cursor_row = sr;
            self.cursor_col = sc;
            self.end_visual();
            self.dirty = true;
        }
    }

    // --- Scroll management ---

    /// Adjust scroll to ensure the cursor is visible within the given viewport height.
    pub fn ensure_cursor_visible(&mut self, viewport_height: usize) {
        if viewport_height == 0 {
            return;
        }
        if self.cursor_row < self.scroll_y {
            self.scroll_y = self.cursor_row;
        }
        if self.cursor_row >= self.scroll_y + viewport_height {
            self.scroll_y = self.cursor_row - viewport_height + 1;
        }
    }

    /// Scroll half a page up.
    pub fn half_page_up(&mut self, viewport_height: usize) {
        let half = viewport_height / 2;
        self.cursor_row = self.cursor_row.saturating_sub(half);
        self.scroll_y = self.scroll_y.saturating_sub(half);
        self.clamp_cursor_col();
    }

    /// Scroll half a page down.
    pub fn half_page_down(&mut self, viewport_height: usize) {
        let half = viewport_height / 2;
        let max_row = self.lines.len().saturating_sub(1);
        self.cursor_row = (self.cursor_row + half).min(max_row);
        self.scroll_y = (self.scroll_y + half).min(max_row);
        self.clamp_cursor_col();
    }

    // --- Search ---

    /// Find next occurrence of `needle` after the cursor position.
    pub fn search_forward(&mut self, needle: &str) -> bool {
        if needle.is_empty() {
            return false;
        }
        // Search in current line after cursor
        let start_col = self.cursor_col + 1;
        if let Some(pos) = self.lines[self.cursor_row]
            .get(start_col..)
            .and_then(|s| s.find(needle))
        {
            self.cursor_col = start_col + pos;
            return true;
        }
        // Search in subsequent lines
        for row in (self.cursor_row + 1)..self.lines.len() {
            if let Some(pos) = self.lines[row].find(needle) {
                self.cursor_row = row;
                self.cursor_col = pos;
                return true;
            }
        }
        // Wrap around
        for row in 0..=self.cursor_row {
            if let Some(pos) = self.lines[row].find(needle) {
                if row == self.cursor_row && pos <= self.cursor_col {
                    continue;
                }
                self.cursor_row = row;
                self.cursor_col = pos;
                return true;
            }
        }
        false
    }

    // --- Internal helpers ---

    fn clamp_cursor_col(&mut self) {
        let max_col = self.current_line_len();
        if self.cursor_col > max_col {
            self.cursor_col = max_col;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_and_content() {
        let mut buf = Buffer::new("test");
        buf.insert_char('H');
        buf.insert_char('i');
        assert_eq!(buf.content(), "Hi");
        assert!(buf.dirty);
    }

    #[test]
    fn test_newline() {
        let mut buf = Buffer::from_text("test", "Hello World");
        buf.cursor_col = 5;
        buf.insert_newline();
        assert_eq!(buf.line_count(), 2);
        assert_eq!(buf.line(0), "Hello");
        assert_eq!(buf.line(1), " World");
    }

    #[test]
    fn test_backspace_join() {
        let mut buf = Buffer::from_text("test", "Line1\nLine2");
        buf.cursor_row = 1;
        buf.cursor_col = 0;
        buf.backspace();
        assert_eq!(buf.line_count(), 1);
        assert_eq!(buf.line(0), "Line1Line2");
    }

    #[test]
    fn test_delete_line() {
        let mut buf = Buffer::from_text("test", "A\nB\nC");
        buf.cursor_row = 1;
        buf.delete_line();
        assert_eq!(buf.line_count(), 2);
        assert_eq!(buf.line(0), "A");
        assert_eq!(buf.line(1), "C");
    }

    #[test]
    fn test_visual_selection_delete() {
        let mut buf = Buffer::from_text("test", "Hello World");
        buf.cursor_col = 0;
        buf.start_visual();
        buf.cursor_col = 5;
        buf.delete_visual_selection();
        assert_eq!(buf.content(), " World");
    }

    #[test]
    fn test_word_motions() {
        let mut buf = Buffer::from_text("test", "hello world foo");
        buf.cursor_col = 0;
        buf.move_word_forward();
        assert_eq!(buf.cursor_col, 6); // start of 'world'
        buf.move_word_forward();
        assert_eq!(buf.cursor_col, 12); // start of 'foo'
        buf.move_word_backward();
        assert_eq!(buf.cursor_col, 6); // back to 'world'
    }

    #[test]
    fn test_search_forward() {
        let mut buf = Buffer::from_text("test", "foo bar baz\nqux foo");
        buf.cursor_col = 0;
        assert!(buf.search_forward("bar"));
        assert_eq!(buf.cursor_row, 0);
        assert_eq!(buf.cursor_col, 4);
        assert!(buf.search_forward("foo"));
        assert_eq!(buf.cursor_row, 1);
        assert_eq!(buf.cursor_col, 4);
    }
}
