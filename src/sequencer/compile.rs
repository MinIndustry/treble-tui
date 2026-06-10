//! Compile treble-lang pattern AST into timed events for playback.

pub use super::transforms::CompileContext;

use super::euclidean::euclidean;
use super::scale::{degree_to_midi, lang_note_to_midi};
use super::transforms;
use super::{PatternSegment, StepEvent};
use treble_lang::ast::mini::{Atom, Group, MiniNotation, Modifier, Sequence, Step};
use treble_lang::ast::program::{PatternDef, PitchRoot, ScaleMode, Transform};

/// A pattern ready for cycle-based playback.
#[derive(Debug, Clone)]
pub struct CompiledPattern {
    pub name: String,
    pub instrument: String,
    pub instrument_idx: usize,
    pub segments: Vec<PatternSegment>,
    /// How many global cycles this pattern spans before repeating.
    pub cycle_factor: f64,
    pub gain: f32,
}

/// Compile active (non-muted) patterns into playback-ready form.
pub fn compile_patterns(
    patterns: &[&PatternDef],
    instrument_idx: impl Fn(&str) -> Option<usize>,
    ctx: &CompileContext,
) -> Vec<CompiledPattern> {
    patterns
        .iter()
        .filter(|p| !p.muted)
        .filter_map(|def| {
            let idx = instrument_idx(&def.instrument)?;
            let (mut segments, mut cycle_factor) = compile_mini(&def.notation, ctx);
            if segments.is_empty() {
                return None;
            }
            transforms::apply_transforms(&mut segments, &mut cycle_factor, &def.transforms, ctx);
            let gain = gain_from_transforms(&def.transforms);
            Some(CompiledPattern {
                name: def.name.clone(),
                instrument: def.instrument.clone(),
                instrument_idx: idx,
                segments,
                cycle_factor,
                gain,
            })
        })
        .collect()
}

fn compile_mini(notation: &MiniNotation, ctx: &CompileContext) -> (Vec<PatternSegment>, f64) {
    let mut slots = Vec::new();
    let mut cycle_factor = 1.0f64;
    expand_sequence(&notation.sequence, 1.0, ctx, &mut slots, &mut cycle_factor);
    merge_holds(&mut slots);
    (slots_to_segments(&slots), cycle_factor)
}

#[derive(Debug, Clone)]
struct Slot {
    weight: f64,
    event: SlotEvent,
    hold: bool,
    drop_chance: bool,
}

#[derive(Debug, Clone)]
enum SlotEvent {
    Rest,
    Trigger,
    Notes(Vec<u8>),
    Alternation(Vec<SlotEvent>),
}

fn expand_sequence(
    seq: &Sequence,
    slot_weight: f64,
    ctx: &CompileContext,
    out: &mut Vec<Slot>,
    cycle_factor: &mut f64,
) {
    if seq.steps.is_empty() {
        return;
    }

    let total_weight: f64 = seq
        .steps
        .iter()
        .map(step_weight)
        .sum::<f64>()
        .max(f64::EPSILON);

    for step in &seq.steps {
        let w = slot_weight * step_weight(step) / total_weight;
        expand_step(step, w, ctx, out, cycle_factor);
    }
}

fn step_weight(step: &Step) -> f64 {
    match step.modifier {
        Some(Modifier::Weight(n)) => n.max(1) as f64,
        _ => 1.0,
    }
}

fn expand_step(
    step: &Step,
    weight: f64,
    ctx: &CompileContext,
    out: &mut Vec<Slot>,
    cycle_factor: &mut f64,
) {
    if let Some(Modifier::Slow(n)) = step.modifier {
        *cycle_factor *= n.max(1) as f64;
    }

    match &step.atom {
        Atom::Hold => {
            out.push(Slot {
                weight,
                event: SlotEvent::Rest,
                hold: true,
                drop_chance: false,
            });
        }
        Atom::Group(group) => expand_group(group, weight, ctx, out, cycle_factor),
        Atom::Alternation(alt) => {
            let options: Vec<SlotEvent> = alt
                .sequence
                .steps
                .iter()
                .map(|s| atom_to_event(&s.atom, ctx))
                .collect();
            out.push(Slot {
                weight,
                event: SlotEvent::Alternation(options),
                hold: false,
                drop_chance: step.modifier == Some(Modifier::Drop),
            });
        }
        atom => {
            let event = atom_to_event(atom, ctx);
            apply_step_modifiers(step, event, weight, out);
        }
    }
}

fn expand_group(
    group: &Group,
    weight: f64,
    ctx: &CompileContext,
    out: &mut Vec<Slot>,
    cycle_factor: &mut f64,
) {
    if group.layers.len() > 1 {
        let mut notes = Vec::new();
        for layer in &group.layers {
            for step in &layer.steps {
                if let SlotEvent::Notes(n) = atom_to_event(&step.atom, ctx) {
                    notes.extend(n);
                }
            }
        }
        out.push(Slot {
            weight,
            event: SlotEvent::Notes(notes),
            hold: false,
            drop_chance: false,
        });
    } else if let Some(layer) = group.layers.first() {
        expand_sequence(layer, weight, ctx, out, cycle_factor);
    }
}

fn apply_step_modifiers(step: &Step, event: SlotEvent, weight: f64, out: &mut Vec<Slot>) {
    let drop = step.modifier == Some(Modifier::Drop);

    match step.modifier {
        Some(Modifier::Repeat(n)) | Some(Modifier::Replicate(n)) => {
            let n = n.max(1);
            let sub_w = weight / n as f64;
            for _ in 0..n {
                out.push(Slot {
                    weight: sub_w,
                    event: event.clone(),
                    hold: false,
                    drop_chance: drop,
                });
            }
        }
        Some(Modifier::Euclidean(beats, steps, offset)) => {
            let pattern = euclidean(beats, steps, offset.unwrap_or(0));
            let sub_w = weight / pattern.len().max(1) as f64;
            for hit in pattern {
                out.push(Slot {
                    weight: sub_w,
                    event: if hit {
                        event.clone()
                    } else {
                        SlotEvent::Rest
                    },
                    hold: false,
                    drop_chance: drop,
                });
            }
        }
        _ => {
            out.push(Slot {
                weight,
                event,
                hold: false,
                drop_chance: drop,
            });
        }
    }
}

fn atom_to_event(atom: &Atom, ctx: &CompileContext) -> SlotEvent {
    match atom {
        Atom::Trigger => SlotEvent::Trigger,
        Atom::Rest => SlotEvent::Rest,
        Atom::Note(n) => SlotEvent::Notes(vec![lang_note_to_midi(n)]),
        Atom::Degree(d) => {
            let (root, mode) = ctx
                .scale
                .unwrap_or((default_root(), ScaleMode::Major));
            SlotEvent::Notes(vec![degree_to_midi(*d, root, mode, 4)])
        }
        Atom::Hold | Atom::Group(_) | Atom::Alternation(_) => SlotEvent::Rest,
    }
}

fn default_root() -> PitchRoot {
    use treble_lang::ast::program::{Accidental, NoteLetter};
    PitchRoot {
        name: NoteLetter::C,
        accidental: Accidental::Natural,
    }
}

fn merge_holds(slots: &mut Vec<Slot>) {
    let mut merged: Vec<Slot> = Vec::with_capacity(slots.len());
    for slot in slots.drain(..) {
        if slot.hold {
            if let Some(prev) = merged.last_mut() {
                prev.weight += slot.weight;
            }
        } else {
            merged.push(slot);
        }
    }
    *slots = merged;
}

fn slots_to_segments(slots: &[Slot]) -> Vec<PatternSegment> {
    let total: f64 = slots.iter().map(|s| s.weight).sum::<f64>().max(f64::EPSILON);
    let mut pos = 0.0;
    let mut segments = Vec::with_capacity(slots.len());

    for slot in slots {
        let start = pos / total;
        pos += slot.weight;
        let end = pos / total;
        segments.push(PatternSegment {
            start,
            end,
            event: slot_event_to_step(&slot.event),
            drop_chance: slot.drop_chance,
        });
    }
    segments
}

fn slot_event_to_step(event: &SlotEvent) -> StepEvent {
    match event {
        SlotEvent::Rest => StepEvent::Rest,
        SlotEvent::Trigger => StepEvent::Trigger,
        SlotEvent::Notes(n) if n.len() == 1 => StepEvent::Note(n[0]),
        SlotEvent::Notes(n) => StepEvent::Chord(n.clone()),
        SlotEvent::Alternation(opts) => {
            StepEvent::Alternation(opts.iter().map(slot_event_to_step).collect())
        }
    }
}

fn gain_from_transforms(transforms: &[Transform]) -> f32 {
    transforms
        .iter()
        .rev()
        .find_map(|t| {
            if let Transform::Gain(g) = t {
                Some(*g as f32)
            } else {
                None
            }
        })
        .unwrap_or(1.0)
        .clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use treble_lang::ast::mini::{MiniNotation, Sequence, Step};
    use treble_lang::ast::program::PatternDef;

    fn pattern(name: &str, instrument: &str, mini: MiniNotation) -> PatternDef {
        PatternDef {
            muted: false,
            name: name.into(),
            instrument: instrument.into(),
            notation: mini,
            transforms: vec![],
        }
    }

    fn mini(steps: Vec<Step>) -> MiniNotation {
        MiniNotation {
            sequence: Sequence { steps },
        }
    }

    fn ctx() -> CompileContext {
        CompileContext { scale: None }
    }

    #[test]
    fn compiles_kick_pattern() {
        let def = pattern(
            "kick",
            "kick",
            mini(vec![
                Step {
                    atom: Atom::Trigger,
                    modifier: None,
                },
                Step {
                    atom: Atom::Rest,
                    modifier: None,
                },
                Step {
                    atom: Atom::Trigger,
                    modifier: None,
                },
                Step {
                    atom: Atom::Rest,
                    modifier: None,
                },
            ]),
        );

        let compiled = compile_patterns(&[&def], |_| Some(0), &ctx());
        assert_eq!(compiled.len(), 1);
        assert_eq!(compiled[0].segments.len(), 4);
    }

    #[test]
    fn repeat_modifier_subdivides() {
        let def = pattern(
            "hats",
            "hihat",
            mini(vec![Step {
                atom: Atom::Trigger,
                modifier: Some(Modifier::Repeat(4)),
            }]),
        );

        let compiled = compile_patterns(&[&def], |_| Some(0), &ctx());
        assert_eq!(compiled[0].segments.len(), 4);
    }

    #[test]
    fn hold_extends_previous() {
        let def = pattern(
            "bass",
            "saw",
            mini(vec![
                Step {
                    atom: Atom::Note(treble_lang::ast::mini::Note {
                        letter: treble_lang::ast::program::NoteLetter::C,
                        accidental: treble_lang::ast::program::Accidental::Natural,
                        octave: 2,
                    }),
                    modifier: None,
                },
                Step {
                    atom: Atom::Hold,
                    modifier: None,
                },
                Step {
                    atom: Atom::Hold,
                    modifier: None,
                },
                Step {
                    atom: Atom::Note(treble_lang::ast::mini::Note {
                        letter: treble_lang::ast::program::NoteLetter::E,
                        accidental: treble_lang::ast::program::Accidental::Flat,
                        octave: 2,
                    }),
                    modifier: None,
                },
            ]),
        );

        let compiled = compile_patterns(&[&def], |_| Some(0), &ctx());
        assert_eq!(compiled[0].segments.len(), 2);
        assert!((compiled[0].segments[0].end - 0.75).abs() < 0.01);
    }
}
