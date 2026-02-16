use crate::panels::{EvalEntry, EvalEntryKind};
use rustic_lang::session::{Delta, Session};

/// Evaluation engine backed by rustic-lang's Session.
///
/// Wraps the rustic-lang [`Session`] and converts its output into
/// display entries for the TUI eval output panel.
pub struct EvalEngine {
    session: Session,
    eval_count: u32,
}

impl EvalEngine {
    pub fn new() -> Self {
        Self {
            session: Session::new(),
            eval_count: 0,
        }
    }

    /// Evaluate the given source code and return display entries.
    pub fn evaluate(&mut self, source: &str) -> Vec<EvalEntry> {
        self.eval_count += 1;
        let timestamp = format!("#{:04}", self.eval_count);
        let mut entries = Vec::new();

        if source.trim().is_empty() {
            entries.push(EvalEntry {
                timestamp,
                kind: EvalEntryKind::Warning,
                message: "Empty source buffer — nothing to evaluate.".to_string(),
            });
            return entries;
        }

        let result = self.session.evaluate(source);

        // Report parse errors
        for err in &result.errors {
            entries.push(EvalEntry {
                timestamp: timestamp.clone(),
                kind: EvalEntryKind::Error,
                message: format!("Line {}: {}", err.location.line, err.message),
            });
        }

        // Report deltas
        for delta in &result.deltas {
            let (action, name) = match delta {
                Delta::Add(n) => ("Added", n.as_str()),
                Delta::Modify(n) => ("Modified", n.as_str()),
                Delta::Remove(n) => ("Removed", n.as_str()),
                Delta::Mute(n) => ("Muted", n.as_str()),
                Delta::Unmute(n) => ("Unmuted", n.as_str()),
            };
            entries.push(EvalEntry {
                timestamp: timestamp.clone(),
                kind: EvalEntryKind::Info,
                message: format!("{} pattern: {}", action, name),
            });
        }

        // Summary
        if result.errors.is_empty() {
            entries.push(EvalEntry {
                timestamp: timestamp.clone(),
                kind: EvalEntryKind::Success,
                message: format!(
                    "OK — {} active, {} muted ({} changes queued)",
                    result.patterns_active,
                    result.patterns_muted,
                    result.deltas.len(),
                ),
            });
        } else {
            entries.push(EvalEntry {
                timestamp: timestamp.clone(),
                kind: EvalEntryKind::Warning,
                message: format!(
                    "{} error(s) — {} active, {} muted",
                    result.errors.len(),
                    result.patterns_active,
                    result.patterns_muted,
                ),
            });
        }

        // Report session state
        entries.push(EvalEntry {
            timestamp,
            kind: EvalEntryKind::Info,
            message: format!(
                "Session: bpm={}, sig={}/{}",
                self.session.bpm, self.session.sig.0, self.session.sig.1
            ),
        });

        entries
    }

    /// Get the current session (for context panel).
    pub fn session(&self) -> &Session {
        &self.session
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eval_empty() {
        let mut engine = EvalEngine::new();
        let entries = engine.evaluate("");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, EvalEntryKind::Warning);
    }

    #[test]
    fn test_eval_valid_pattern() {
        let mut engine = EvalEngine::new();
        let entries = engine.evaluate("kick kick \"x ~ x ~\"");
        assert!(entries.iter().any(|e| e.kind == EvalEntryKind::Success));
        assert!(entries.iter().any(|e| e.message.contains("Added")));
    }

    #[test]
    fn test_eval_with_error() {
        let mut engine = EvalEngine::new();
        let entries = engine.evaluate("this is broken ???");
        assert!(entries.iter().any(|e| e.kind == EvalEntryKind::Error));
    }

    #[test]
    fn test_eval_detects_changes() {
        let mut engine = EvalEngine::new();
        engine.evaluate("kick kick \"x ~ x ~\"");
        let entries = engine.evaluate("kick kick \"x x x x\"");
        assert!(entries.iter().any(|e| e.message.contains("Modified")));
    }

    #[test]
    fn test_eval_reports_bpm() {
        let mut engine = EvalEngine::new();
        let entries = engine.evaluate("bpm 140\nkick kick \"x ~ x ~\"");
        assert!(entries.iter().any(|e| e.message.contains("bpm=140")));
    }
}
