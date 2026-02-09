/// Represents a pending vim motion or operator.
/// Used for multi-key sequences like `d`, `c`, `y` followed by a motion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Motion {
    /// Waiting for a motion after `d` (delete).
    Delete,
    /// Waiting for a motion after `y` (yank).
    Yank,
    /// Waiting for a motion after `c` (change).
    Change,
    /// Waiting for second key in `g` sequences (gg, etc.)
    G,
}
