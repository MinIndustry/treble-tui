/// Vim-like editor modes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// Navigation and command mode (default).
    Normal,
    /// Text insertion mode.
    Insert,
    /// Visual (character-wise) selection mode.
    Visual,
    /// Command-line mode (`:` commands).
    Command,
    /// Search mode (`/` to search forward).
    Search,
}

impl Mode {
    pub fn label(&self) -> &'static str {
        match self {
            Mode::Normal => "NORMAL",
            Mode::Insert => "INSERT",
            Mode::Visual => "VISUAL",
            Mode::Command => "COMMAND",
            Mode::Search => "SEARCH",
        }
    }
}

impl std::fmt::Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

/// The command-line input state (for `:` commands and `/` search).
#[derive(Debug, Clone)]
pub struct CommandLine {
    /// The text entered so far (excluding the prefix `:` or `/`).
    pub input: String,
    /// Cursor position within the command-line input.
    pub cursor: usize,
}

impl CommandLine {
    pub fn new() -> Self {
        Self {
            input: String::new(),
            cursor: 0,
        }
    }

    pub fn clear(&mut self) {
        self.input.clear();
        self.cursor = 0;
    }

    pub fn insert_char(&mut self, ch: char) {
        self.input.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            let prev = self.input[..self.cursor]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.input.remove(prev);
            self.cursor = prev;
        }
    }

    pub fn take(&mut self) -> String {
        let s = self.input.clone();
        self.clear();
        s
    }
}
