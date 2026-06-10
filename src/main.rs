mod editor;
mod eval;
mod layout;
mod panels;
mod sequencer;

use std::io;
use std::time::{Duration, Instant};

use clap::Parser;
use treble_tui::logging::{LogCli, LogConfig, emit_init_failure, init as init_logging};

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use editor::{Buffer, CommandLine, Mode, Motion};
use eval::EvalEngine;
use layout::ColumnLayout;
use panels::{
    CodeEditorPanel, ContextInfo, ContextPanel, EvalEntry, EvalEntryKind, EvalOutputPanel,
    InstrumentInfo, SequencePatternView, SequenceVizPanel,
};
use treble_lang::ast::program::PatternDef;
use sequencer::{InstrumentRegistry, Sequencer, SequenceView, build_snapshot};

/// Target UI render rate (frames per second).
const RENDER_TPS: u64 = 30;
/// Sequencer poll rate — decoupled from rendering for tighter timing.
const SEQUENCER_TPS: u64 = 120;

/// Application state.
struct App {
    /// The code editor buffer (Column 1).
    code_buffer: Buffer,
    /// Read-only buffer for the context panel (Column 3) — future use.
    eval_entries: Vec<EvalEntry>,
    /// Scroll position for the eval output panel.
    eval_scroll: usize,
    /// Context information for the reference panel.
    context: ContextInfo,
    /// The eval engine (stub).
    eval_engine: EvalEngine,

    /// Current editor mode.
    mode: Mode,
    /// Command-line input state.
    command_line: CommandLine,
    /// Last search query for `/` and `n` repeat.
    last_search: String,
    /// Pending operator motion (d, y, c).
    pending_motion: Option<Motion>,
    /// Yank register (clipboard).
    yank_register: String,

    /// Column layout manager.
    columns: ColumnLayout,

    /// Status message shown in the bottom bar (transient).
    status_message: Option<(String, Instant)>,

    /// Whether the application should quit.
    should_quit: bool,

    /// The treble backend application.
    treble_app: Option<treble::prelude::App>,

    /// Instrument name registry for the audio graph.
    instrument_registry: InstrumentRegistry,

    /// Pattern sequencer driving audio playback.
    sequencer: Sequencer,

    /// Whether the audio engine has successfully started.
    engine_running: bool,

    /// Cached sequence visualizer state (rebuilt each frame).
    sequence_view: Option<SequenceView>,
}

impl App {
    fn new() -> Self {
        let sample_code = include_str!("sample_code.rt");

        let treble_app = {
            let mut app = treble::prelude::App::new();
            if let Ok(erx) = app.start(treble::audio::EventFilter::default()) {
                std::thread::spawn(move || {
                    while let Ok(event) = erx.recv() {
                        log::trace!(target: "treble_tui::backend", "backend event: {event:?}");
                    }
                });
                Some(app)
            } else {
                log::error!(target: "treble_tui::backend", "failed to start treble audio engine");
                None
            }
        };

        let engine_running = treble_app.is_some();

        let mut app = Self {
            code_buffer: Buffer::from_text("score.rt", sample_code),
            eval_entries: Vec::new(),
            eval_scroll: 0,
            context: ContextInfo::default(),
            eval_engine: EvalEngine::new(),
            mode: Mode::Normal,
            command_line: CommandLine::new(),
            last_search: String::new(),
            pending_motion: None,
            yank_register: String::new(),
            columns: ColumnLayout::new(),
            status_message: None,
            should_quit: false,
            treble_app,
            instrument_registry: InstrumentRegistry::default(),
            sequencer: Sequencer::new(),
            engine_running,
            sequence_view: None,
        };
        app.update_context_keybindings();
        app
    }

    fn quit(&mut self) {
        self.sequencer.stop();
        if let Some(mut app) = self.treble_app.take() {
            if let Err(e) = app.stop() {
                log::error!(target: "treble_tui::backend", "failed to stop treble audio engine: {e:?}");
            } else {
                log::info!(target: "treble_tui::backend", "treble audio engine stopped");
            }
        }
    }

    fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some((msg.into(), Instant::now()));
    }

    fn evaluate_buffer(&mut self) {
        let source = self.code_buffer.content();
        let entries = self.eval_engine.evaluate(&source);
        let success = entries.iter().any(|e| e.kind == EvalEntryKind::Success);
        self.eval_entries.extend(entries);
        // Auto-scroll to bottom
        let viewport_guess = 20usize;
        if self.eval_entries.len() > viewport_guess {
            self.eval_scroll = self.eval_entries.len() - viewport_guess;
        }
        self.code_buffer.dirty = false;
        if success {
            self.sync_sequencer_after_eval();
            self.set_status("Evaluation complete.");
        } else {
            self.set_status("Evaluation finished with errors.");
        }
        self.update_context_keybindings();
    }

    /// Register instruments and queue a playback snapshot after a successful eval.
    fn sync_sequencer_after_eval(&mut self) {
        let session = self.eval_engine.session();
        let bpm = session.bpm;
        let sig = session.sig;
        let scale = session.scale;
        let patterns: Vec<_> = session.active_patterns().into_iter().cloned().collect();
        if patterns.is_empty() {
            return;
        }

        let instrument_names: Vec<String> = patterns.iter().map(|p| p.instrument.clone()).collect();
        let pattern_refs: Vec<&PatternDef> = patterns.iter().collect();

        if let Some(app) = self.treble_app.as_mut() {
            let added = self
                .instrument_registry
                .ensure(app, instrument_names.into_iter());
            if added {
                if let Err(e) = app.recompile() {
                    log::error!(
                        target: "treble_tui::sequencer",
                        "failed to recompile audio graph: {e:?}"
                    );
                }
            }
        }

        if let Some(snapshot) =
            build_snapshot(bpm, sig, scale, pattern_refs, &self.instrument_registry)
        {
            self.sequencer.queue_snapshot(snapshot);
        }
    }

    fn tick_sequencer(&mut self) {
        let Some(app) = self.treble_app.as_ref() else {
            return;
        };
        if self.sequencer.tick(Instant::now(), app) {
            self.eval_engine.apply_pending();
        }
    }

    fn execute_command(&mut self, cmd: &str) {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        match parts.first().copied() {
            Some("w") | Some("write") => {
                self.evaluate_buffer();
            }
            Some("q") | Some("quit") => {
                self.should_quit = true;
            }
            Some("wq") => {
                self.evaluate_buffer();
                self.should_quit = true;
            }
            Some("q!") => {
                self.should_quit = true;
            }
            Some("clear") => {
                self.eval_entries.clear();
                self.eval_scroll = 0;
                self.set_status("Output cleared.");
            }
            Some("reset") => {
                self.columns.reset();
                self.set_status("Layout reset to 40-30-30.");
            }
            Some("e") | Some("edit") => {
                if let Some(name) = parts.get(1) {
                    self.code_buffer = Buffer::new(*name);
                    self.set_status(format!("New buffer: {}", name));
                } else {
                    self.set_status("Usage: :e <filename>");
                }
            }
            Some(other) => {
                self.set_status(format!("Unknown command: {}", other));
            }
            None => {}
        }
    }

    fn update_context_keybindings(&mut self) {
        let bindings = match self.mode {
            Mode::Normal => vec![
                ("i".into(), "Enter insert mode".into()),
                ("v".into(), "Enter visual mode".into()),
                (":".into(), "Command mode".into()),
                ("/".into(), "Search forward".into()),
                ("h/j/k/l".into(), "Move cursor".into()),
                ("w/b/e".into(), "Word motions".into()),
                ("0/$".into(), "Line start/end".into()),
                ("gg/G".into(), "Top/bottom".into()),
                ("dd".into(), "Delete line".into()),
                ("D".into(), "Delete to EOL".into()),
                ("yy".into(), "Yank line".into()),
                ("p".into(), "Paste below".into()),
                ("o/O".into(), "Open line below/above".into()),
                ("Ctrl+S".into(), "Evaluate (save)".into()),
                ("Tab".into(), "Next column".into()),
                ("Shift+Tab".into(), "Prev column".into()),
                ("Ctrl+>".into(), "Grow column".into()),
                ("Ctrl+<".into(), "Shrink column".into()),
                ("Ctrl+Q".into(), "Quit".into()),
            ],
            Mode::Insert => vec![
                ("Esc".into(), "Return to normal".into()),
                ("Ctrl+S".into(), "Evaluate (save)".into()),
                ("Enter".into(), "New line".into()),
                ("Backspace".into(), "Delete char back".into()),
            ],
            Mode::Visual => vec![
                ("Esc".into(), "Cancel selection".into()),
                ("d/x".into(), "Delete selection".into()),
                ("y".into(), "Yank selection".into()),
                ("h/j/k/l".into(), "Extend selection".into()),
            ],
            Mode::Command => vec![
                ("Enter".into(), "Execute command".into()),
                ("Esc".into(), "Cancel".into()),
                (":w".into(), "Evaluate buffer".into()),
                (":q".into(), "Quit".into()),
                (":wq".into(), "Eval & quit".into()),
                (":clear".into(), "Clear output".into()),
                (":reset".into(), "Reset layout".into()),
            ],
            Mode::Search => vec![
                ("Enter".into(), "Search".into()),
                ("Esc".into(), "Cancel".into()),
                ("n".into(), "(after) Next match".into()),
            ],
        };
        self.context.keybindings = bindings;

        let session = self.eval_engine.session();
        if !self.engine_running {
            self.context.engine_status = "No audio backend".to_string();
        } else if self.sequencer.is_running() {
            self.context.engine_status = format!(
                "Playing — {} patterns @ {} bpm, {}/{}",
                self.sequencer.active_pattern_count(),
                session.bpm,
                session.sig.0,
                session.sig.1,
            );
        } else {
            self.context.engine_status = "Engine ready — evaluate to start".to_string();
        }

        self.context.instruments = self
            .instrument_registry
            .registered_names()
            .map(|name| InstrumentInfo {
                name: name.clone(),
                active: self
                    .eval_engine
                    .session()
                    .active_patterns()
                    .iter()
                    .any(|p| p.instrument == *name),
                voice_count: if InstrumentRegistry::is_percussion(name) {
                    1
                } else {
                    8
                },
            })
            .collect();
    }

    /// Handle a keyboard event based on the current mode.
    fn handle_key(&mut self, key: KeyEvent) {
        // Global shortcuts (available in all modes)
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('q') => {
                    self.should_quit = true;
                    return;
                }
                KeyCode::Char('s') => {
                    self.evaluate_buffer();
                    if self.mode == Mode::Insert {
                        // Stay in insert mode after Ctrl+S
                    } else {
                        self.mode = Mode::Normal;
                    }
                    self.update_context_keybindings();
                    return;
                }
                _ => {}
            }
        }

        match self.mode {
            Mode::Normal => self.handle_normal_mode(key),
            Mode::Insert => self.handle_insert_mode(key),
            Mode::Visual => self.handle_visual_mode(key),
            Mode::Command => self.handle_command_mode(key),
            Mode::Search => self.handle_search_mode(key),
        }
        self.update_context_keybindings();
    }

    fn handle_normal_mode(&mut self, key: KeyEvent) {
        // Handle pending motions first (dd, yy, gg, etc.)
        if let Some(motion) = self.pending_motion.take() {
            match motion {
                Motion::Delete => match key.code {
                    KeyCode::Char('d') => {
                        self.yank_register = self.code_buffer.current_line().to_string();
                        self.code_buffer.delete_line();
                    }
                    KeyCode::Char('w') => {
                        // delete word — simplified: delete to next word boundary
                        self.code_buffer.delete_to_end_of_line();
                    }
                    _ => {
                        self.set_status("Unknown motion for d");
                    }
                },
                Motion::Yank => match key.code {
                    KeyCode::Char('y') => {
                        self.yank_register = self.code_buffer.current_line().to_string();
                        self.set_status("Line yanked.");
                    }
                    _ => {
                        self.set_status("Unknown motion for y");
                    }
                },
                Motion::Change => match key.code {
                    KeyCode::Char('c') => {
                        self.yank_register = self.code_buffer.current_line().to_string();
                        self.code_buffer.delete_line();
                        self.code_buffer.open_line_above();
                        self.mode = Mode::Insert;
                    }
                    _ => {
                        self.set_status("Unknown motion for c");
                    }
                },
                Motion::G => match key.code {
                    KeyCode::Char('g') => {
                        self.code_buffer.move_to_top();
                    }
                    _ => {
                        self.set_status("Unknown g motion");
                    }
                },
            }
            return;
        }

        // Column switching with Tab / Shift+Tab
        if key.code == KeyCode::Tab {
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                self.columns.focus_prev();
            } else {
                self.columns.focus_next();
            }
            return;
        }
        if key.code == KeyCode::BackTab {
            self.columns.focus_prev();
            return;
        }

        // Column resizing with Ctrl+Right / Ctrl+Left
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Right | KeyCode::Char('l')
                    if key
                        .modifiers
                        .contains(KeyModifiers::CONTROL | KeyModifiers::SHIFT) =>
                {
                    self.columns.grow_focused();
                    let r = self.columns.ratios();
                    self.set_status(format!("Layout: {}-{}-{}", r[0], r[1], r[2]));
                    return;
                }
                KeyCode::Left | KeyCode::Char('h')
                    if key
                        .modifiers
                        .contains(KeyModifiers::CONTROL | KeyModifiers::SHIFT) =>
                {
                    self.columns.shrink_focused();
                    let r = self.columns.ratios();
                    self.set_status(format!("Layout: {}-{}-{}", r[0], r[1], r[2]));
                    return;
                }
                _ => {}
            }
        }

        // Column resize with Ctrl+Right/Left
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Right => {
                    self.columns.grow_focused();
                    let r = self.columns.ratios();
                    self.set_status(format!("Layout: {}-{}-{}", r[0], r[1], r[2]));
                    return;
                }
                KeyCode::Left => {
                    self.columns.shrink_focused();
                    let r = self.columns.ratios();
                    self.set_status(format!("Layout: {}-{}-{}", r[0], r[1], r[2]));
                    return;
                }
                _ => {}
            }
        }

        // Column direct focus with Alt+1/2/3
        if key.modifiers.contains(KeyModifiers::ALT) {
            match key.code {
                KeyCode::Char('1') => {
                    self.columns.focus(0);
                    return;
                }
                KeyCode::Char('2') => {
                    self.columns.focus(1);
                    return;
                }
                KeyCode::Char('3') => {
                    self.columns.focus(2);
                    return;
                }
                _ => {}
            }
        }

        // Ctrl+U / Ctrl+D for half-page scroll
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('u') => {
                    self.code_buffer.half_page_up(20);
                    return;
                }
                KeyCode::Char('d') => {
                    self.code_buffer.half_page_down(20);
                    return;
                }
                _ => {}
            }
        }

        match key.code {
            // Mode switches
            KeyCode::Char('i') => self.mode = Mode::Insert,
            KeyCode::Char('a') => {
                self.code_buffer.move_right(1);
                self.mode = Mode::Insert;
            }
            KeyCode::Char('A') => {
                self.code_buffer.move_to_line_end();
                self.mode = Mode::Insert;
            }
            KeyCode::Char('I') => {
                self.code_buffer.move_to_first_non_blank();
                self.mode = Mode::Insert;
            }
            KeyCode::Char('v') => {
                self.code_buffer.start_visual();
                self.mode = Mode::Visual;
            }
            KeyCode::Char(':') => {
                self.mode = Mode::Command;
                self.command_line.clear();
            }
            KeyCode::Char('/') => {
                self.mode = Mode::Search;
                self.command_line.clear();
            }

            // Motions
            KeyCode::Char('h') | KeyCode::Left => self.code_buffer.move_left(1),
            KeyCode::Char('j') | KeyCode::Down => self.code_buffer.move_down(1),
            KeyCode::Char('k') | KeyCode::Up => self.code_buffer.move_up(1),
            KeyCode::Char('l') | KeyCode::Right => self.code_buffer.move_right(1),

            KeyCode::Char('w') => self.code_buffer.move_word_forward(),
            KeyCode::Char('b') => self.code_buffer.move_word_backward(),
            KeyCode::Char('e') => self.code_buffer.move_word_end(),

            KeyCode::Char('0') => self.code_buffer.move_to_line_start(),
            KeyCode::Char('^') => self.code_buffer.move_to_first_non_blank(),
            KeyCode::Char('$') => self.code_buffer.move_to_line_end(),

            KeyCode::Char('G') => self.code_buffer.move_to_bottom(),
            KeyCode::Char('g') => self.pending_motion = Some(Motion::G),

            // Editing
            KeyCode::Char('x') => self.code_buffer.delete_char(),
            KeyCode::Char('r') => {
                // Replace mode: next char typed replaces current
                // Simplified: just delete and enter insert
                self.code_buffer.delete_char();
                self.mode = Mode::Insert;
            }

            KeyCode::Char('o') => {
                self.code_buffer.open_line_below();
                self.mode = Mode::Insert;
            }
            KeyCode::Char('O') => {
                self.code_buffer.open_line_above();
                self.mode = Mode::Insert;
            }

            KeyCode::Char('d') => self.pending_motion = Some(Motion::Delete),
            KeyCode::Char('D') => {
                self.code_buffer.delete_to_end_of_line();
            }
            KeyCode::Char('y') => self.pending_motion = Some(Motion::Yank),
            KeyCode::Char('c') => self.pending_motion = Some(Motion::Change),

            KeyCode::Char('p') => {
                // Paste below
                if !self.yank_register.is_empty() {
                    self.code_buffer.open_line_below();
                    for ch in self.yank_register.clone().chars() {
                        self.code_buffer.insert_char(ch);
                    }
                }
            }
            KeyCode::Char('P') => {
                // Paste above
                if !self.yank_register.is_empty() {
                    self.code_buffer.open_line_above();
                    for ch in self.yank_register.clone().chars() {
                        self.code_buffer.insert_char(ch);
                    }
                }
            }

            KeyCode::Char('n') => {
                // Repeat last search
                if !self.last_search.is_empty() {
                    let query = self.last_search.clone();
                    if !self.code_buffer.search_forward(&query) {
                        self.set_status(format!("Pattern not found: {}", query));
                    }
                }
            }

            _ => {}
        }
    }

    fn handle_insert_mode(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                // Move cursor back one if possible (vim behavior)
                self.code_buffer.move_left(1);
                self.mode = Mode::Normal;
            }
            KeyCode::Enter => self.code_buffer.insert_newline(),
            KeyCode::Backspace => self.code_buffer.backspace(),
            KeyCode::Delete => self.code_buffer.delete_char(),
            KeyCode::Left => self.code_buffer.move_left(1),
            KeyCode::Right => self.code_buffer.move_right(1),
            KeyCode::Up => self.code_buffer.move_up(1),
            KeyCode::Down => self.code_buffer.move_down(1),
            KeyCode::Tab => {
                // Insert 4 spaces
                for _ in 0..4 {
                    self.code_buffer.insert_char(' ');
                }
            }
            KeyCode::Char(ch) => self.code_buffer.insert_char(ch),
            _ => {}
        }
    }

    fn handle_visual_mode(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.code_buffer.end_visual();
                self.mode = Mode::Normal;
            }
            // Motions (extend selection)
            KeyCode::Char('h') | KeyCode::Left => self.code_buffer.move_left(1),
            KeyCode::Char('j') | KeyCode::Down => self.code_buffer.move_down(1),
            KeyCode::Char('k') | KeyCode::Up => self.code_buffer.move_up(1),
            KeyCode::Char('l') | KeyCode::Right => self.code_buffer.move_right(1),
            KeyCode::Char('w') => self.code_buffer.move_word_forward(),
            KeyCode::Char('b') => self.code_buffer.move_word_backward(),
            KeyCode::Char('e') => self.code_buffer.move_word_end(),
            KeyCode::Char('0') => self.code_buffer.move_to_line_start(),
            KeyCode::Char('$') => self.code_buffer.move_to_line_end(),
            KeyCode::Char('G') => self.code_buffer.move_to_bottom(),

            // Actions on selection
            KeyCode::Char('d') | KeyCode::Char('x') => {
                self.code_buffer.delete_visual_selection();
                self.mode = Mode::Normal;
            }
            KeyCode::Char('y') => {
                if let Some((sr, sc, er, ec)) = self.code_buffer.visual_range() {
                    // Simple yank: just grab the text
                    let content = self.code_buffer.content();
                    let lines: Vec<&str> = content.lines().collect();
                    let mut yanked = String::new();
                    for row in sr..=er {
                        if row < lines.len() {
                            let line = lines[row];
                            let start = if row == sr { sc.min(line.len()) } else { 0 };
                            let end = if row == er {
                                ec.min(line.len())
                            } else {
                                line.len()
                            };
                            if start <= end && start <= line.len() {
                                yanked.push_str(&line[start..end.min(line.len())]);
                            }
                            if row < er {
                                yanked.push('\n');
                            }
                        }
                    }
                    self.yank_register = yanked;
                    self.set_status("Selection yanked.");
                }
                self.code_buffer.end_visual();
                self.mode = Mode::Normal;
            }
            _ => {}
        }
    }

    fn handle_command_mode(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.command_line.clear();
                self.mode = Mode::Normal;
            }
            KeyCode::Enter => {
                let cmd = self.command_line.take();
                self.mode = Mode::Normal;
                self.execute_command(&cmd);
            }
            KeyCode::Backspace => {
                self.command_line.backspace();
                if self.command_line.input.is_empty() {
                    self.mode = Mode::Normal;
                }
            }
            KeyCode::Char(ch) => {
                self.command_line.insert_char(ch);
            }
            _ => {}
        }
    }

    fn handle_search_mode(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.command_line.clear();
                self.mode = Mode::Normal;
            }
            KeyCode::Enter => {
                let query = self.command_line.take();
                self.mode = Mode::Normal;
                if !query.is_empty() {
                    self.last_search = query.clone();
                    if !self.code_buffer.search_forward(&query) {
                        self.set_status(format!("Pattern not found: {}", query));
                    }
                }
            }
            KeyCode::Backspace => {
                self.command_line.backspace();
                if self.command_line.input.is_empty() {
                    self.mode = Mode::Normal;
                }
            }
            KeyCode::Char(ch) => {
                self.command_line.insert_char(ch);
            }
            _ => {}
        }
    }
}

// --- UI rendering ---

fn render(app: &mut App, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    terminal.draw(|frame| {
        let size = frame.area();

        // Main layout: columns area + status bar + command line
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),    // columns
                Constraint::Length(1), // status bar
                Constraint::Length(1), // command / mode line
            ])
            .split(size);

        let columns_area = main_chunks[0];
        let status_area = main_chunks[1];
        let cmdline_area = main_chunks[2];

        // --- Three-column layout ---
        let col_rects = app.columns.split(columns_area);
        let viewport_height = col_rects[0].height.saturating_sub(2) as usize; // subtract borders

        // Ensure cursor is visible
        app.code_buffer.ensure_cursor_visible(viewport_height);

        // Column 1: Code editor
        let editor_panel = CodeEditorPanel::new(
            &app.code_buffer,
            &app.mode,
            app.columns.focused_index() == 0,
        );
        frame.render_widget(editor_panel, col_rects[0]);

        // Column 2: Eval output + sequence visualizer
        let pattern_count = app
            .sequence_view
            .as_ref()
            .map(|v| v.patterns.len())
            .unwrap_or(1);
        let viz_height = SequenceVizPanel::height_for_patterns(pattern_count.min(6));

        let col2_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(viz_height)])
            .split(col_rects[1]);

        app.sequence_view = app.sequencer.sequence_view(std::time::Instant::now());

        let pattern_views: Vec<SequencePatternView> = app
            .sequence_view
            .as_ref()
            .map(|v| {
                v.patterns
                    .iter()
                    .map(|p| SequencePatternView {
                        pattern_name: p.pattern_name.clone(),
                        instrument: p.instrument.clone(),
                        phase: p.phase,
                        current_step: p.current_step,
                        steps: p
                            .steps
                            .iter()
                            .map(|s| (s.label.clone(), s.start, s.end))
                            .collect(),
                    })
                    .collect()
            })
            .unwrap_or_default();

        let output_panel = EvalOutputPanel::new(
            &app.eval_entries,
            app.eval_scroll,
            app.columns.focused_index() == 1,
        );
        frame.render_widget(output_panel, col2_chunks[0]);

        let (global_cycle, cycle_phase) = app
            .sequence_view
            .as_ref()
            .map(|v| (v.global_cycle, v.cycle_phase))
            .unwrap_or((0, 0.0));

        let viz_panel = SequenceVizPanel::new(
            &pattern_views,
            global_cycle,
            cycle_phase,
            app.columns.focused_index() == 1,
        );
        frame.render_widget(viz_panel, col2_chunks[1]);

        // Column 3: Context / reference
        let context_panel = ContextPanel::new(&app.context, app.columns.focused_index() == 2);
        frame.render_widget(context_panel, col_rects[2]);

        // --- Status bar ---
        let ratios = app.columns.ratios();
        let status_msg = app
            .status_message
            .as_ref()
            .filter(|(_, t)| t.elapsed() < Duration::from_secs(5))
            .map(|(m, _)| m.as_str())
            .unwrap_or("");

        let status_line = Line::from(vec![
            Span::styled(
                format!(" {} ", app.mode),
                Style::default()
                    .fg(Color::Black)
                    .bg(match app.mode {
                        Mode::Normal => Color::Cyan,
                        Mode::Insert => Color::Green,
                        Mode::Visual => Color::Magenta,
                        Mode::Command => Color::Yellow,
                        Mode::Search => Color::Yellow,
                    })
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" {} ", app.code_buffer.name),
                Style::default().fg(Color::White).bg(Color::DarkGray),
            ),
            Span::styled(
                format!(
                    " {}:{} ",
                    app.code_buffer.cursor_row + 1,
                    app.code_buffer.cursor_col + 1
                ),
                Style::default().fg(Color::White).bg(Color::DarkGray),
            ),
            Span::styled(
                format!(" [{}-{}-{}] ", ratios[0], ratios[1], ratios[2]),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                format!(" Col {} ", app.columns.focused_index() + 1),
                Style::default().fg(Color::Cyan),
            ),
            Span::styled(
                format!(" {} ", status_msg),
                Style::default().fg(Color::Yellow),
            ),
        ]);

        let status_bar =
            Paragraph::new(status_line).style(Style::default().bg(Color::Rgb(30, 30, 30)));
        frame.render_widget(status_bar, status_area);

        // --- Command / mode line ---
        let cmdline_content = match app.mode {
            Mode::Command => format!(":{}", app.command_line.input),
            Mode::Search => format!("/{}", app.command_line.input),
            _ => String::new(),
        };
        let cmdline_widget = Paragraph::new(cmdline_content)
            .style(Style::default().fg(Color::White).bg(Color::Black));
        frame.render_widget(cmdline_widget, cmdline_area);
    })?;
    Ok(())
}

// --- Main entry point ---

fn main() -> io::Result<()> {
    let log_cli = LogCli::parse();
    let log_config = LogConfig::from_cli(&log_cli);
    let _log_guard = init_logging(&log_config).unwrap_or_else(|err| {
        emit_init_failure(&err);
        std::process::exit(1);
    });

    log::info!(target: "treble_tui", "starting application");

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    let render_interval = Duration::from_millis(1000 / RENDER_TPS);
    let sequencer_interval = Duration::from_millis(1000 / SEQUENCER_TPS);
    let mut last_render = Instant::now();
    let mut last_sequencer = Instant::now();

    // Initial welcome message
    app.eval_entries.push(EvalEntry {
        timestamp: "#0000".to_string(),
        kind: EvalEntryKind::Info,
        message: "Welcome to Treble TUI — live coding environment.".to_string(),
    });
    app.eval_entries.push(EvalEntry {
        timestamp: "#0000".to_string(),
        kind: EvalEntryKind::Info,
        message: "Press :w or Ctrl+S to evaluate. :q to quit.".to_string(),
    });

    // Main event loop
    loop {
        let now = Instant::now();

        if app.sequencer.is_running() && now.duration_since(last_sequencer) >= sequencer_interval {
            last_sequencer = now;
            app.tick_sequencer();
        }

        if now.duration_since(last_render) >= render_interval {
            last_render = now;
            render(&mut app, &mut terminal)?;

            if app.should_quit {
                app.quit();
                break;
            }
        }

        let until_sequencer = sequencer_interval.saturating_sub(now.duration_since(last_sequencer));
        let until_render = render_interval.saturating_sub(now.duration_since(last_render));
        let wait = until_sequencer.min(until_render).max(Duration::from_millis(1));

        if event::poll(wait)?
            && let Event::Key(key) = event::read()?
        {
            app.handle_key(key);
            if app.sequencer.is_running() {
                app.tick_sequencer();
                last_sequencer = Instant::now();
            }
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    log::info!(target: "treble_tui", "application shut down");
    Ok(())
}
