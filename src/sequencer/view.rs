//! Playback state for the sequence visualizer.

use std::time::Instant;

use treble::Note;

use super::{
    CompiledPattern, PlaybackSnapshot, Sequencer, StepEvent, find_segment, pattern_phase,
};

/// One step cell in a pattern timeline.
#[derive(Debug, Clone)]
pub struct StepCell {
    pub label: String,
    pub start: f64,
    pub end: f64,
}

/// Live view of a single pattern sequence + playhead.
#[derive(Debug, Clone)]
pub struct PatternSequenceView {
    pub pattern_name: String,
    pub instrument: String,
    pub instrument_idx: usize,
    /// Position within this pattern's cycle (0.0–1.0).
    pub phase: f64,
    pub current_step: usize,
    pub steps: Vec<StepCell>,
}

/// Full visualizer state for all playing patterns.
#[derive(Debug, Clone)]
pub struct SequenceView {
    pub global_cycle: u64,
    /// Position within the current measure (0.0–1.0).
    pub cycle_phase: f64,
    pub patterns: Vec<PatternSequenceView>,
}

impl Sequencer {
    /// Snapshot of pattern sequences and playhead positions for the UI.
    pub fn sequence_view(&self, now: Instant) -> Option<SequenceView> {
        let playing = self.playing.as_ref()?;
        Some(build_sequence_view(
            playing,
            self.cycle_start,
            now,
            self.global_cycle,
        ))
    }
}

fn build_sequence_view(
    playing: &PlaybackSnapshot,
    cycle_start: Instant,
    now: Instant,
    global_cycle: u64,
) -> SequenceView {
    let cycle = playing.cycle_duration();
    let elapsed = now.saturating_duration_since(cycle_start);
    let cycle_phase =
        (elapsed.as_secs_f64() / cycle.as_secs_f64().max(f64::EPSILON)).clamp(0.0, 1.0);

    let patterns = playing
        .patterns
        .iter()
        .map(|p| pattern_sequence_view(p, cycle_phase, global_cycle))
        .collect();

    SequenceView {
        global_cycle,
        cycle_phase,
        patterns,
    }
}

fn pattern_sequence_view(
    pattern: &CompiledPattern,
    global_phase: f64,
    global_cycle: u64,
) -> PatternSequenceView {
    let phase = pattern_phase(global_phase, pattern.cycle_factor);
    let current_step = find_segment(&pattern.segments, phase);
    let steps = pattern
        .segments
        .iter()
        .map(|seg| StepCell {
            label: event_display_label(&seg.event, global_cycle),
            start: seg.start,
            end: seg.end,
        })
        .collect();

    PatternSequenceView {
        pattern_name: pattern.name.clone(),
        instrument: pattern.instrument.clone(),
        instrument_idx: pattern.instrument_idx,
        phase,
        current_step,
        steps,
    }
}

/// Short label for a step event (timeline cell).
pub fn event_display_label(event: &StepEvent, cycle: u64) -> String {
    match event {
        StepEvent::Rest => "~".into(),
        StepEvent::Trigger => "x".into(),
        StepEvent::Note(midi) => note_label(*midi),
        StepEvent::Chord(midis) if midis.is_empty() => "~".into(),
        StepEvent::Chord(midis) if midis.len() == 1 => note_label(midis[0]),
        StepEvent::Chord(midis) => {
            let parts: Vec<_> = midis.iter().map(|m| note_label(*m)).collect();
            parts.join("+")
        }
        StepEvent::Alternation(opts) if opts.is_empty() => "~".into(),
        StepEvent::Alternation(opts) => {
            event_display_label(&opts[cycle as usize % opts.len()], cycle)
        }
    }
}

fn note_label(midi: u8) -> String {
    Note::from_midi(midi).to_string().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_labels() {
        assert_eq!(event_display_label(&StepEvent::Trigger, 0), "x");
        assert_eq!(event_display_label(&StepEvent::Rest, 0), "~");
        assert_eq!(event_display_label(&StepEvent::Note(60), 0), "c4");
    }
}
