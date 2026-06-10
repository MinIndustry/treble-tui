//! Scale-degree resolution and quantisation.

use treble_lang::ast::program::{Accidental, NoteLetter, PitchRoot, ScaleMode};

/// Resolve a scale degree to a MIDI note number.
///
/// Degree 0 is the root in octave 4 by default; negative degrees descend.
pub fn degree_to_midi(degree: i32, root: PitchRoot, mode: ScaleMode, base_octave: u8) -> u8 {
    let intervals = mode_intervals(mode);
    if intervals.is_empty() {
        return 60;
    }

    let len = intervals.len() as i32;
    let mut d = degree;
    let mut octave_shift = 0i32;
    while d < 0 {
        d += len;
        octave_shift -= 1;
    }
    octave_shift += d / len;
    let idx = (d % len) as usize;
    let semitone = root_semitone(root) + intervals[idx];
    let octave = (base_octave as i32 + octave_shift).clamp(0, 8) as u8;
    (octave as i32 * 12 + semitone + 12).clamp(0, 127) as u8
}

/// Quantise a MIDI note to the nearest scale tone.
pub fn quantise_midi(midi: u8, root: PitchRoot, mode: ScaleMode) -> u8 {
    let intervals = mode_intervals(mode);
    if intervals.is_empty() {
        return midi;
    }

    let root_st = root_semitone(root).rem_euclid(12);
    let pitch_class = (midi as i32 % 12) - root_st;
    let pc = pitch_class.rem_euclid(12);

    let mut best = intervals[0];
    let mut best_dist = i32::MAX;
    for &iv in &intervals {
        let dist = (pc - iv).abs().min((pc - iv + 12).abs()).min((pc - iv - 12).abs());
        if dist < best_dist {
            best_dist = dist;
            best = iv;
        }
    }

    let octave = (midi / 12) as i32;
    (octave * 12 + root_st + best).clamp(0, 127) as u8
}

fn root_semitone(root: PitchRoot) -> i32 {
    let base = match root.name {
        NoteLetter::C => 0,
        NoteLetter::D => 2,
        NoteLetter::E => 4,
        NoteLetter::F => 5,
        NoteLetter::G => 7,
        NoteLetter::A => 9,
        NoteLetter::B => 11,
    };
    base + match root.accidental {
        Accidental::Natural => 0,
        Accidental::Sharp => 1,
        Accidental::DoubleSharp => 2,
        Accidental::Flat => -1,
        Accidental::DoubleFlat => -2,
    }
}

fn mode_intervals(mode: ScaleMode) -> Vec<i32> {
    match mode {
        ScaleMode::Major => vec![0, 2, 4, 5, 7, 9, 11],
        ScaleMode::Minor | ScaleMode::Aeolian => vec![0, 2, 3, 5, 7, 8, 10],
        ScaleMode::Dorian => vec![0, 2, 3, 5, 7, 9, 10],
        ScaleMode::Phrygian => vec![0, 1, 3, 5, 7, 8, 10],
        ScaleMode::Lydian => vec![0, 2, 4, 6, 7, 9, 11],
        ScaleMode::Mixolydian => vec![0, 2, 4, 5, 7, 9, 10],
        ScaleMode::Locrian => vec![0, 1, 3, 5, 6, 8, 10],
        ScaleMode::Chromatic => (0..12).collect(),
        ScaleMode::Pentatonic => vec![0, 2, 4, 7, 9],
        ScaleMode::Blues => vec![0, 3, 5, 6, 7, 10],
    }
}

/// Convert a language AST note to MIDI.
pub fn lang_note_to_midi(note: &treble_lang::ast::mini::Note) -> u8 {
    let semitone = match note.letter {
        NoteLetter::C => 0,
        NoteLetter::D => 2,
        NoteLetter::E => 4,
        NoteLetter::F => 5,
        NoteLetter::G => 7,
        NoteLetter::A => 9,
        NoteLetter::B => 11,
    } + match note.accidental {
        Accidental::Natural => 0,
        Accidental::Sharp => 1,
        Accidental::DoubleSharp => 2,
        Accidental::Flat => -1,
        Accidental::DoubleFlat => -2,
    };

    let octave = note.octave as i32;
    (octave * 12 + semitone + 12).clamp(0, 127) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn degree_zero_is_root() {
        let root = PitchRoot {
            name: NoteLetter::C,
            accidental: Accidental::Natural,
        };
        let midi = degree_to_midi(0, root, ScaleMode::Major, 4);
        assert_eq!(midi, 60);
    }
}
