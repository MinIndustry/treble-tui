//! Transform pipeline applied after mini-notation compilation.

use treble_lang::ast::program::{ArpMode, PitchRoot, ScaleMode, Transform};

use super::{PatternSegment, StepEvent};
use crate::sequencer::scale::quantise_midi;

/// Context available during pattern compilation.
#[derive(Debug, Clone, Copy)]
pub struct CompileContext {
    pub scale: Option<(PitchRoot, ScaleMode)>,
}

pub fn apply_transforms(
    segments: &mut Vec<PatternSegment>,
    cycle_factor: &mut f64,
    transforms: &[Transform],
    ctx: &CompileContext,
) {
    for transform in transforms {
        apply_transform(segments, cycle_factor, transform, ctx);
    }
}

fn apply_transform(
    segments: &mut Vec<PatternSegment>,
    cycle_factor: &mut f64,
    transform: &Transform,
    ctx: &CompileContext,
) {
    match transform {
        Transform::Rev => {
            let total = segments.last().map(|s| s.end).unwrap_or(1.0);
            let mut reversed: Vec<_> = segments
                .iter()
                .rev()
                .map(|s| PatternSegment {
                    start: total - s.end,
                    end: total - s.start,
                    event: s.event.clone(),
                    drop_chance: s.drop_chance,
                })
                .collect();
            reversed.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());
            *segments = reversed;
        }
        Transform::Fast(n) => {
            let n = (*n).max(f64::EPSILON);
            *cycle_factor /= n;
            for seg in segments.iter_mut() {
                seg.start /= n;
                seg.end /= n;
            }
        }
        Transform::Slow(n) => {
            let n = (*n).max(f64::EPSILON);
            *cycle_factor *= n;
            for seg in segments.iter_mut() {
                seg.start /= n;
                seg.end /= n;
            }
        }
        Transform::Oct(offset) => {
            for seg in segments.iter_mut() {
                transpose_segment(seg, *offset * 12);
            }
        }
        Transform::Gain(g) => {
            let _ = g;
        }
        Transform::Scale(root, mode) => {
            for seg in segments.iter_mut() {
                quantise_segment(seg, *root, *mode);
            }
        }
        Transform::Arp(mode) => {
            arpeggiate(segments, *mode);
        }
        Transform::Every(_n, inner) => {
            apply_transform(segments, cycle_factor, inner, ctx);
        }
        Transform::Lpf(_) | Transform::Hpf(_) | Transform::Delay(_, _) | Transform::Reverb(_) => {
            log::debug!(
                target: "treble_tui::sequencer",
                "FX transforms are not yet applied at runtime — use instrument graphs"
            );
        }
    }
}

fn transpose_segment(seg: &mut PatternSegment, semitones: i32) {
    seg.event = match &seg.event {
        StepEvent::Note(n) => StepEvent::Note(transpose_midi(*n, semitones)),
        StepEvent::Chord(notes) => {
            StepEvent::Chord(notes.iter().map(|n| transpose_midi(*n, semitones)).collect())
        }
        StepEvent::Alternation(opts) => StepEvent::Alternation(
            opts.iter()
                .map(|e| transpose_event(e, semitones))
                .collect(),
        ),
        other => other.clone(),
    };
}

fn transpose_event(event: &StepEvent, semitones: i32) -> StepEvent {
    match event {
        StepEvent::Note(n) => StepEvent::Note(transpose_midi(*n, semitones)),
        StepEvent::Chord(n) => {
            StepEvent::Chord(n.iter().map(|x| transpose_midi(*x, semitones)).collect())
        }
        other => other.clone(),
    }
}

fn transpose_midi(midi: u8, semitones: i32) -> u8 {
    ((midi as i32 + semitones).clamp(0, 127)) as u8
}

fn quantise_segment(seg: &mut PatternSegment, root: PitchRoot, mode: ScaleMode) {
    seg.event = match &seg.event {
        StepEvent::Note(n) => StepEvent::Note(quantise_midi(*n, root, mode)),
        StepEvent::Chord(notes) => {
            StepEvent::Chord(notes.iter().map(|n| quantise_midi(*n, root, mode)).collect())
        }
        StepEvent::Alternation(opts) => StepEvent::Alternation(
            opts.iter()
                .map(|e| quantise_event(e, root, mode))
                .collect(),
        ),
        other => other.clone(),
    };
}

fn quantise_event(event: &StepEvent, root: PitchRoot, mode: ScaleMode) -> StepEvent {
    match event {
        StepEvent::Note(n) => StepEvent::Note(quantise_midi(*n, root, mode)),
        StepEvent::Chord(n) => {
            StepEvent::Chord(n.iter().map(|x| quantise_midi(*x, root, mode)).collect())
        }
        other => other.clone(),
    }
}

fn arpeggiate(segments: &mut Vec<PatternSegment>, mode: ArpMode) {
    let mut out = Vec::new();
    for seg in segments.drain(..) {
        if let StepEvent::Chord(notes) = seg.event {
            let ordered = order_chord(&notes, mode);
            let len = seg.end - seg.start;
            let step = len / ordered.len().max(1) as f64;
            for (i, midi) in ordered.into_iter().enumerate() {
                out.push(PatternSegment {
                    start: seg.start + step * i as f64,
                    end: seg.start + step * (i as f64 + 1.0),
                    event: StepEvent::Note(midi),
                    drop_chance: seg.drop_chance,
                });
            }
        } else {
            out.push(seg);
        }
    }
    *segments = out;
}

fn order_chord(notes: &[u8], mode: ArpMode) -> Vec<u8> {
    let mut sorted = notes.to_vec();
    sorted.sort_unstable();
    match mode {
        ArpMode::Up => sorted,
        ArpMode::Down => {
            sorted.reverse();
            sorted
        }
        ArpMode::UpDown => {
            if sorted.len() <= 1 {
                return sorted;
            }
            let mut updown = sorted.clone();
            updown.extend(sorted.iter().rev().skip(1).take(sorted.len().saturating_sub(2)));
            updown
        }
        ArpMode::Random => {
            let pivot = sorted.iter().map(|n| *n as u32).sum::<u32>() as usize % sorted.len();
            sorted.rotate_left(pivot);
            sorted
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use treble_lang::ast::program::Transform;

    #[test]
    fn rev_reverses_segments() {
        let mut segments = vec![
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
        apply_transforms(
            &mut segments,
            &mut 1.0,
            &[Transform::Rev],
            &CompileContext { scale: None },
        );
        assert!(matches!(segments[0].event, StepEvent::Rest));
    }
}
