//! Animating units in the live-sim render: sequence selection, time evaluation, and vertex skinning.

use mdx::Model;

/// Select a sequence by name, trying exact match first then fallback sequences.
/// Returns the sequence index.
pub fn select_sequence(model: &Model, sequence_name: &str, fallback_names: &[&str]) -> Option<usize> {
    // Try the primary sequence name (case-insensitive)
    if let Some(idx) = model.find_sequence(sequence_name) {
        return Some(idx);
    }

    // Try fallback sequence names
    for fallback in fallback_names {
        if let Some(idx) = model.find_sequence(fallback) {
            return Some(idx);
        }
    }

    None
}

/// Compute the animation time (milliseconds within the sequence) from a unit's
/// game time offset, clamped to the sequence length.
pub fn animation_time_ms(model: &Model, seq_idx: Option<usize>, time_offset_ms: u32) -> Option<u32> {
    let seq = model.sequences.get(seq_idx?)?;
    let duration_ms = (seq.end - seq.start) as u32;
    if duration_ms == 0 {
        return Some(0);
    }
    Some(time_offset_ms % duration_ms)
}
