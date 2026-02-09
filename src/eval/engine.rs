use crate::panels::{EvalEntry, EvalEntryKind};

/// Stub evaluation engine.
///
/// In the future, this will connect to the Rustic audio backend
/// and the rustic-lang compiler. For now, it provides a mock
/// read-eval-execute loop that validates syntax superficially.
pub struct EvalEngine {
    /// Eval counter for timestamps
    eval_count: u32,
}

impl EvalEngine {
    pub fn new() -> Self {
        Self { eval_count: 0 }
    }

    /// Evaluate the given source code and return output entries.
    ///
    /// Currently a stub that does basic validation:
    /// - Checks for balanced braces/brackets/parens
    /// - Reports line count and char count
    /// - Returns mock compilation feedback
    pub fn evaluate(&mut self, source: &str) -> Vec<EvalEntry> {
        self.eval_count += 1;
        let timestamp = format!("#{:04}", self.eval_count);
        let mut entries = Vec::new();

        if source.trim().is_empty() {
            entries.push(EvalEntry {
                timestamp: timestamp.clone(),
                kind: EvalEntryKind::Warning,
                message: "Empty source buffer — nothing to evaluate.".to_string(),
            });
            return entries;
        }

        // Basic metrics
        let line_count = source.lines().count();
        let char_count = source.len();
        entries.push(EvalEntry {
            timestamp: timestamp.clone(),
            kind: EvalEntryKind::Info,
            message: format!("Evaluating: {} lines, {} bytes", line_count, char_count),
        });

        // Check balanced delimiters
        match check_balanced(source) {
            Ok(()) => {}
            Err(err) => {
                entries.push(EvalEntry {
                    timestamp: timestamp.clone(),
                    kind: EvalEntryKind::Error,
                    message: err,
                });
                return entries;
            }
        }

        // TODO: wire into rustic-lang compiler
        // let ast = rustic_lang::parse(source);
        // let score = rustic_lang::compile(ast);
        // send score to audio backend

        entries.push(EvalEntry {
            timestamp: timestamp.clone(),
            kind: EvalEntryKind::Success,
            message: "Parse OK (stub) — no audio backend connected.".to_string(),
        });

        // Report any score/instrument keywords found (preview of future functionality)
        let keywords_found: Vec<&str> = ["score", "staff", "instrument", "measure", "note", "bpm"]
            .iter()
            .filter(|kw| source.contains(**kw))
            .copied()
            .collect();

        if !keywords_found.is_empty() {
            entries.push(EvalEntry {
                timestamp,
                kind: EvalEntryKind::Info,
                message: format!("Keywords detected: {}", keywords_found.join(", ")),
            });
        }

        entries
    }
}

/// Check that braces, brackets, and parens are balanced.
fn check_balanced(source: &str) -> Result<(), String> {
    let mut stack: Vec<(char, usize, usize)> = Vec::new(); // (char, line, col)
    for (line_idx, line) in source.lines().enumerate() {
        for (col_idx, ch) in line.chars().enumerate() {
            match ch {
                '(' | '[' | '{' => stack.push((ch, line_idx + 1, col_idx + 1)),
                ')' | ']' | '}' => {
                    let expected = match ch {
                        ')' => '(',
                        ']' => '[',
                        '}' => '{',
                        _ => unreachable!(),
                    };
                    match stack.pop() {
                        Some((open, _, _)) if open == expected => {}
                        Some((open, l, c)) => {
                            return Err(format!(
                                "Mismatched delimiter: '{}' at {}:{} closed by '{}' at {}:{}",
                                open,
                                l,
                                c,
                                ch,
                                line_idx + 1,
                                col_idx + 1
                            ));
                        }
                        None => {
                            return Err(format!(
                                "Unexpected closing '{}' at {}:{}",
                                ch,
                                line_idx + 1,
                                col_idx + 1
                            ));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    if let Some((ch, l, c)) = stack.last() {
        return Err(format!("Unclosed '{}' at {}:{}", ch, l, c));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_balanced_ok() {
        assert!(check_balanced("fn main() { let x = [1, 2]; }").is_ok());
    }

    #[test]
    fn test_balanced_mismatch() {
        assert!(check_balanced("fn main() { let x = [1, 2); }").is_err());
    }

    #[test]
    fn test_eval_empty() {
        let mut engine = EvalEngine::new();
        let entries = engine.evaluate("");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, EvalEntryKind::Warning);
    }

    #[test]
    fn test_eval_valid() {
        let mut engine = EvalEngine::new();
        let entries = engine.evaluate("score { staff { note C4 } }");
        assert!(entries.iter().any(|e| e.kind == EvalEntryKind::Success));
    }
}
