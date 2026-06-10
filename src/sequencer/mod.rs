//! Pattern sequencer — drives the treble audio engine from live session state.

mod compile;
mod euclidean;
mod instruments;
mod scale;
mod transforms;
mod view;

pub use compile::{CompileContext, CompiledPattern, compile_patterns};
pub use instruments::InstrumentRegistry;
pub use view::SequenceView;

use std::collections::HashMap;
use std::time::{Duration, Instant};

use treble::Note;
use treble::NOTES;
use treble::prelude::App;
use treble_lang::ast::program::{PatternDef, PitchRoot, ScaleMode};

/// A timed segment within one pattern cycle (0.0–1.0).
#[derive(Debug, Clone, PartialEq)]
pub struct PatternSegment {
    pub start: f64,
    pub end: f64,
    pub event: StepEvent,
    pub drop_chance: bool,
}

/// Musical content of a segment.
#[derive(Debug, Clone, PartialEq)]
pub enum StepEvent {
    Rest,
    Trigger,
    Note(u8),
    Chord(Vec<u8>),
    Alternation(Vec<StepEvent>),
}

/// Snapshot of transport + patterns used for playback.
#[derive(Debug, Clone)]
pub struct PlaybackSnapshot {
    pub bpm: u32,
    pub sig: (u8, u8),
    pub patterns: Vec<CompiledPattern>,
}

impl PlaybackSnapshot {
    pub fn cycle_duration(&self) -> Duration {
        cycle_duration(self.bpm, self.sig)
    }
}

/// Live activity on an audio-graph instrument slot.
#[derive(Debug, Default, Clone)]
pub struct InstrumentActivity {
    pub trigger: bool,
    pub midis: Vec<u8>,
}

/// Drives pattern playback with strict loop-boundary quantisation.
#[derive(Debug)]
pub struct Sequencer {
    playing: Option<PlaybackSnapshot>,
    queued: Option<PlaybackSnapshot>,
    running: bool,
    cycle_start: Instant,
    global_cycle: u64,
    /// Last active segment index per pattern.
    last_segment: HashMap<String, usize>,
    active: HashMap<usize, InstrumentActivity>,
}

impl Default for Sequencer {
    fn default() -> Self {
        Self::new()
    }
}

impl Sequencer {
    pub fn new() -> Self {
        Self {
            playing: None,
            queued: None,
            running: false,
            cycle_start: Instant::now(),
            global_cycle: 0,
            last_segment: HashMap::new(),
            active: HashMap::new(),
        }
    }

    /// Current per-slot activity (indexed by `instrument_idx`).
    pub fn instrument_activity(&self) -> &HashMap<usize, InstrumentActivity> {
        &self.active
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn active_pattern_count(&self) -> usize {
        self.playing
            .as_ref()
            .map(|p| p.patterns.len())
            .unwrap_or(0)
    }

    pub fn queue_snapshot(&mut self, snapshot: PlaybackSnapshot) {
        if self.playing.is_none() {
            self.playing = Some(snapshot);
            self.cycle_start = Instant::now();
            self.global_cycle = 0;
            self.last_segment.clear();
            self.active.clear();
            self.running = true;
            log::info!(target: "treble_tui::sequencer", "playback started");
        } else {
            self.queued = Some(snapshot);
            log::info!(target: "treble_tui::sequencer", "changes queued for loop boundary");
        }
    }

    pub fn tick(&mut self, now: Instant, app: &App) -> bool {
        if !self.running {
            return false;
        }

        let mut loop_boundary_applied = false;
        let mut wraps = 0u32;
        const MAX_WRAPS: u32 = 32;

        loop {
            let Some(playing) = self.playing.clone() else {
                return loop_boundary_applied;
            };

            let cycle = playing.cycle_duration();
            let elapsed = now.saturating_duration_since(self.cycle_start);

            if elapsed >= cycle {
                if wraps >= MAX_WRAPS {
                    self.resync_after_stall(now, cycle, app, &playing);
                    loop_boundary_applied = true;
                    let elapsed = now.saturating_duration_since(self.cycle_start);
                    let position =
                        elapsed.as_secs_f64() / cycle.as_secs_f64().max(f64::EPSILON);
                    if let Some(current) = self.playing.clone() {
                        self.process_segments_at_position(app, &current, position);
                    }
                    break;
                }

                self.release_all_segments(app, &playing);
                self.apply_loop_boundary(cycle);
                loop_boundary_applied = true;
                wraps += 1;

                if now.saturating_duration_since(self.cycle_start) >= cycle {
                    continue;
                }
            }

            let elapsed = now.saturating_duration_since(self.cycle_start);
            let position =
                elapsed.as_secs_f64() / cycle.as_secs_f64().max(f64::EPSILON);
            self.process_segments_at_position(app, &playing, position);
            break;
        }

        loop_boundary_applied
    }

    fn process_segments_at_position(
        &mut self,
        app: &App,
        playing: &PlaybackSnapshot,
        position: f64,
    ) {
        for pattern in &playing.patterns {
            let pattern_phase = pattern_phase(position, pattern.cycle_factor);
            let seg_idx = find_segment(&pattern.segments, pattern_phase);
            let prev = self.last_segment.get(&pattern.name).copied();

            if prev == Some(seg_idx) {
                continue;
            }

            if let Some(prev_idx) = prev {
                self.release_segment(app, pattern, prev_idx);
            }

            self.fire_segment(app, pattern, seg_idx);
            self.last_segment.insert(pattern.name.clone(), seg_idx);
        }
    }

    fn release_all_segments(&mut self, app: &App, playing: &PlaybackSnapshot) {
        for pattern in &playing.patterns {
            if let Some(prev_idx) = self.last_segment.get(&pattern.name).copied() {
                self.release_segment(app, pattern, prev_idx);
            }
        }
    }

    /// Jump forward when the UI thread stalled across many cycles.
    fn resync_after_stall(
        &mut self,
        now: Instant,
        cycle: Duration,
        app: &App,
        playing: &PlaybackSnapshot,
    ) {
        let elapsed = now.saturating_duration_since(self.cycle_start);
        let cycle_secs = cycle.as_secs_f64().max(f64::EPSILON);
        let whole_cycles = (elapsed.as_secs_f64() / cycle_secs).floor() as u64;
        if whole_cycles == 0 {
            return;
        }

        self.release_all_segments(app, playing);
        if let Some(queued) = self.queued.take() {
            log::info!(
                target: "treble_tui::sequencer",
                "applying queued changes during stall resync (patterns={})",
                queued.patterns.len(),
            );
            self.playing = Some(queued);
        }

        self.global_cycle += whole_cycles;
        self.cycle_start +=
            Duration::from_secs_f64(cycle_secs * whole_cycles as f64);
        self.last_segment.clear();
        self.active.clear();
    }

    pub fn stop(&mut self) {
        self.running = false;
        self.playing = None;
        self.queued = None;
        self.last_segment.clear();
        self.active.clear();
    }

    fn apply_loop_boundary(&mut self, cycle: Duration) {
        if let Some(queued) = self.queued.take() {
            log::info!(
                target: "treble_tui::sequencer",
                "applying queued changes at loop boundary (bpm={}, sig={}/{}, patterns={})",
                queued.bpm,
                queued.sig.0,
                queued.sig.1,
                queued.patterns.len(),
            );
            self.playing = Some(queued);
        }

        self.global_cycle += 1;
        self.cycle_start += cycle;
        self.last_segment.clear();
        self.active.clear();
    }

    fn fire_segment(&mut self, app: &App, pattern: &CompiledPattern, seg_idx: usize) {
        let Some(seg) = pattern.segments.get(seg_idx) else {
            return;
        };

        if seg.drop_chance && should_drop(self.global_cycle, &pattern.name, seg_idx) {
            return;
        }

        let event = resolve_event(&seg.event, self.global_cycle);
        let velocity = pattern.gain.clamp(0.0, 1.0);

        let idx = pattern.instrument_idx;

        match event {
            StepEvent::Rest => {}
            StepEvent::Trigger => {
                let note = Note::new(NOTES::A, 4);
                let _ = app.note_on(idx, note, velocity);
                let slot = self.active.entry(idx).or_default();
                slot.trigger = true;
                slot.midis.clear();
            }
            StepEvent::Note(midi) => {
                let note = Note::from_midi(midi);
                let _ = app.note_on(idx, note, velocity);
                let slot = self.active.entry(idx).or_default();
                slot.trigger = false;
                slot.midis = vec![midi];
            }
            StepEvent::Chord(ref midis) => {
                for &midi in midis {
                    let note = Note::from_midi(midi);
                    let _ = app.note_on(idx, note, velocity);
                }
                let slot = self.active.entry(idx).or_default();
                slot.trigger = false;
                slot.midis = midis.clone();
            }
            StepEvent::Alternation(_) => {}
        }
    }

    fn release_segment(&mut self, app: &App, pattern: &CompiledPattern, seg_idx: usize) {
        let Some(seg) = pattern.segments.get(seg_idx) else {
            return;
        };

        let event = resolve_event(&seg.event, self.global_cycle);
        let idx = pattern.instrument_idx;
        match event {
            StepEvent::Trigger => {
                let note = Note::new(NOTES::A, 4);
                let _ = app.note_off(idx, note);
            }
            StepEvent::Note(midi) => {
                let note = Note::from_midi(midi);
                let _ = app.note_off(idx, note);
            }
            StepEvent::Chord(ref midis) => {
                for &midi in midis {
                    let note = Note::from_midi(midi);
                    let _ = app.note_off(idx, note);
                }
            }
            _ => {}
        }

        if let Some(slot) = self.active.get_mut(&idx) {
            slot.trigger = false;
            slot.midis.clear();
        }
    }
}

fn resolve_event(event: &StepEvent, cycle: u64) -> StepEvent {
    match event {
        StepEvent::Alternation(opts) if !opts.is_empty() => {
            opts[cycle as usize % opts.len()].clone()
        }
        other => other.clone(),
    }
}

pub(crate) fn find_segment(segments: &[PatternSegment], position: f64) -> usize {
    for (i, seg) in segments.iter().enumerate() {
        if position >= seg.start && position < seg.end {
            return i;
        }
    }
    segments.len().saturating_sub(1)
}

pub(crate) fn pattern_phase(global_position: f64, cycle_factor: f64) -> f64 {
    if cycle_factor <= 1.0 {
        global_position
    } else {
        (global_position * cycle_factor).fract()
    }
}

fn should_drop(cycle: u64, pattern: &str, seg_idx: usize) -> bool {
    let mut hash = cycle;
    for b in pattern.bytes() {
        hash = hash.wrapping_mul(31).wrapping_add(b as u64);
    }
    (hash.wrapping_add(seg_idx as u64) % 2) == 0
}

/// Build a playback snapshot from session patterns.
pub fn build_snapshot(
    bpm: u32,
    sig: (u8, u8),
    scale: Option<(PitchRoot, ScaleMode)>,
    patterns: Vec<&PatternDef>,
    registry: &InstrumentRegistry,
) -> Option<PlaybackSnapshot> {
    let ctx = CompileContext { scale };
    let compiled = compile_patterns(
        &patterns,
        |name| registry.instrument_idx(name),
        &ctx,
    );

    if compiled.is_empty() {
        return None;
    }

    Some(PlaybackSnapshot {
        bpm,
        sig,
        patterns: compiled,
    })
}

fn cycle_duration(bpm: u32, sig: (u8, u8)) -> Duration {
    let (num, den) = sig;
    let bpm = bpm.max(1) as f64;
    let quarter_beats = num as f64 * 4.0 / den.max(1) as f64;
    let secs = quarter_beats * 60.0 / bpm;
    Duration::from_secs_f64(secs.max(0.01))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycle_duration_4_4_at_120() {
        let d = cycle_duration(120, (4, 4));
        assert!((d.as_secs_f64() - 2.0).abs() < 0.001);
    }

    #[test]
    fn find_segment_by_position() {
        let segments = vec![
            PatternSegment {
                start: 0.0,
                end: 0.5,
                event: StepEvent::Trigger,
                drop_chance: false,
            },
            PatternSegment {
                start: 0.5,
                end: 1.0,
                event: StepEvent::Rest,
                drop_chance: false,
            },
        ];
        assert_eq!(find_segment(&segments, 0.25), 0);
        assert_eq!(find_segment(&segments, 0.75), 1);
    }

    #[test]
    fn loop_boundary_preserves_phase_within_cycle() {
        let cycle = Duration::from_secs(2);
        let t0 = Instant::now();
        let mut seq = Sequencer::new();
        seq.cycle_start = t0;
        seq.apply_loop_boundary(cycle);
        assert_eq!(seq.cycle_start, t0 + cycle);

        // 50 ms into the new cycle → phase ≈ 0.025
        let now = t0 + cycle + Duration::from_millis(50);
        let elapsed = now.saturating_duration_since(seq.cycle_start);
        let position = elapsed.as_secs_f64() / cycle.as_secs_f64();
        assert!((position - 0.025).abs() < 0.01);
    }
}
