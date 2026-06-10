//! Map DSL instrument names to Treble `Instrument` instances.

use std::collections::HashMap;

use treble::instruments::prelude::{Clap, HiHat, Kick, Rim, Snare, Synth, Tom};
use treble::instruments::Instrument;
use treble::prelude::App;

const PERCUSSION: &[&str] = &["kick", "snare", "hihat", "clap", "rim", "tom"];

/// Registry of instrument names → slot indices in the treble audio graph.
#[derive(Debug, Default)]
pub struct InstrumentRegistry {
    name_to_idx: HashMap<String, usize>,
}

impl InstrumentRegistry {
    pub fn instrument_idx(&self, name: &str) -> Option<usize> {
        self.name_to_idx.get(name).copied()
    }

    pub fn is_percussion(name: &str) -> bool {
        PERCUSSION.contains(&name)
    }

    /// Ensure every named instrument exists in the audio graph.
    ///
    /// Returns `true` if new instruments were added (caller should `recompile`).
    pub fn ensure(&mut self, app: &mut App, names: impl IntoIterator<Item = String>) -> bool {
        let mut added = false;
        for name in names {
            if self.name_to_idx.contains_key(&name) {
                continue;
            }
            let instrument = match build_instrument(&name) {
                Ok(inst) => inst,
                Err(e) => {
                    log::error!(
                        target: "treble_tui::sequencer",
                        "failed to build instrument '{name}': {e}"
                    );
                    continue;
                }
            };
            let idx = app.add_instrument(instrument);
            log::info!(
                target: "treble_tui::sequencer",
                "registered instrument '{name}' at idx {idx}"
            );
            self.name_to_idx.insert(name, idx);
            added = true;
        }
        added
    }

    pub fn registered_names(&self) -> impl Iterator<Item = &String> {
        self.name_to_idx.keys()
    }

    /// All registered instruments sorted by graph slot index.
    pub fn entries(&self) -> Vec<(&str, usize)> {
        let mut entries: Vec<_> = self
            .name_to_idx
            .iter()
            .map(|(name, idx)| (name.as_str(), *idx))
            .collect();
        entries.sort_by_key(|(_, idx)| *idx);
        entries
    }
}

fn build_instrument(name: &str) -> Result<Box<dyn Instrument>, String> {
    match name {
        "kick" => Ok(Box::new(Kick::new())),
        "snare" => Ok(Box::new(Snare::new())),
        "clap" => Ok(Box::new(Clap::new())),
        "rim" => Ok(Box::new(Rim::new())),
        "hihat" => HiHat::new()
            .map(|h| Box::new(h) as Box<dyn Instrument>)
            .map_err(|e| e.to_string()),
        "tom" => Ok(Box::new(Tom::new())),
        "sine" | "saw" | "square" | "triangle" | "piano" | "bass" | "pad" | "pluck" | "bell" => {
            Ok(Box::new(Synth::from_name(name)))
        }
        other => {
            log::warn!(
                target: "treble_tui::sequencer",
                "unknown instrument '{other}', using piano synth"
            );
            Ok(Box::new(Synth::from_name("piano")))
        }
    }
}
