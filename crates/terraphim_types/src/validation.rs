//! Core type invariants: validated construction, stable identity derivation
//! and UTF-8-safe preview helpers.
//!
//! This module provides the shared invariant machinery referenced by the
//! core document, graph and identifier types:
//!
//! - [`stable_id`]: deterministic, process-independent identifier derivation
//!   (FNV-1a over UTF-8 bytes). Unlike a counter, the same value always
//!   yields the same ID across insertion order, reload and process restart.
//! - [`truncate_utf8_safe`]: preview truncation that never panics on valid
//!   UTF-8 and counts Unicode scalar values (characters), not bytes.
//! - [`ValidationError`]: typed errors returned when invalid external values
//!   are rejected instead of silently accepted.

/// Errors produced when a value violates a documented type invariant.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ValidationError {
    /// A score value was outside the documented 0.0-1.0 range.
    #[error("score out of range: `{field}` must be within 0.0-1.0, got {value}")]
    ScoreOutOfRange {
        /// Name of the offending field.
        field: &'static str,
        /// The invalid value that was supplied.
        value: f64,
    },
    /// A required string field was empty or whitespace-only.
    #[error("empty value: `{0}` must not be empty or whitespace-only")]
    EmptyValue(&'static str),
}

/// Derive a stable 64-bit identifier from content.
///
/// Uses FNV-1a over the UTF-8 bytes of `value`. The result is deterministic
/// across insertion order, target platform and process restarts; it must be
/// preferred over counter-based IDs whenever identity needs to survive
/// persistence.
pub fn stable_id(value: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Truncate `text` to at most `max_chars` Unicode scalar values.
///
/// The limit counts characters (Unicode scalar values), not bytes, and the
/// returned string is always valid UTF-8. Never panics: if the limit falls
/// inside a multi-byte character, the whole character is dropped.
///
/// # Examples
///
/// ```
/// use terraphim_types::validation::truncate_utf8_safe;
///
/// assert_eq!(truncate_utf8_safe("héllo", 3), "hél");
/// // A multi-byte character straddling the byte limit is not split.
/// assert_eq!(truncate_utf8_safe("aé", 2), "aé");
/// assert_eq!(truncate_utf8_safe("héllo", 10), "héllo");
/// ```
pub fn truncate_utf8_safe(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    text.chars().take(max_chars).collect()
}

/// A short, UTF-8-safe preview of `text` limited to `max_chars` characters,
/// with `suffix` appended when truncation occurred.
pub fn preview(text: &str, max_chars: usize, suffix: &str) -> String {
    if text.chars().count() <= max_chars {
        text.to_string()
    } else {
        let mut out = truncate_utf8_safe(text, max_chars);
        out.push_str(suffix);
        out
    }
}

/// Validated construction for score triples (0.0-1.0 per dimension).
///
/// Returns a [`ValidationError`] instead of accepting partial or out-of-range
/// values, so the same invariant applies at construction and deserialisation
/// boundaries.
pub fn validate_score(field: &'static str, value: f64) -> Result<f64, ValidationError> {
    if value.is_finite() && (0.0..=1.0).contains(&value) {
        Ok(value)
    } else {
        Err(ValidationError::ScoreOutOfRange { field, value })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_id_is_deterministic_across_processes() {
        // FNV-1a vectors are fixed constants, so the ID is stable across
        // processes, insertion order and platforms.
        assert_eq!(stable_id("rust"), stable_id("rust"));
        assert_eq!(stable_id(""), 0xcbf2_9ce4_8422_2325);
        assert_ne!(stable_id("rust"), stable_id("Rust"));
        assert_ne!(stable_id("a"), stable_id("b"));
    }

    #[test]
    fn stable_id_is_independent_of_runtime_counter_state() {
        // Deriving twice with other derivations interleaved never changes the
        // outcome (a counter-based scheme would produce different IDs).
        let a = stable_id("machine learning");
        let _ = stable_id("noise-1");
        let _ = stable_id("noise-2");
        assert_eq!(a, stable_id("machine learning"));
    }

    #[test]
    fn truncate_handles_multibyte_and_boundaries() {
        // ASCII
        assert_eq!(truncate_utf8_safe("hello", 4), "hell");
        // Multibyte: é is two bytes; byte-index slicing would panic or split.
        assert_eq!(truncate_utf8_safe("héllo", 3), "hél");
        // Emoji (4-byte scalar)
        assert_eq!(truncate_utf8_safe("a🙂b", 2), "a🙂");
        // Combining mark: e + U+0301 counts as two scalars
        assert_eq!(truncate_utf8_safe("e\u{0301}x", 1), "e");
        // Limit beyond length
        assert_eq!(truncate_utf8_safe("hi", 100), "hi");
        // Zero limit
        assert_eq!(truncate_utf8_safe("hi", 0), "");
        // Empty input
        assert_eq!(truncate_utf8_safe("", 5), "");
    }

    #[test]
    fn truncate_never_panics_on_arbitrary_unicode() {
        // Property-style sweep over tricky scalars: every call must return
        // valid UTF-8 of at most max_chars scalars.
        let samples = [
            "e\u{0301}\u{200d}🙂 emoji",
            "\u{0}a\u{7f}b\u{10ffff}",
            "日本語のテキスト",
            "混合 mixed текст",
        ];
        for s in samples {
            for max in 0..=s.chars().count() + 2 {
                let out = truncate_utf8_safe(s, max);
                assert!(std::str::from_utf8(out.as_bytes()).is_ok());
                assert!(out.chars().count() <= max);
            }
        }
    }

    #[test]
    fn preview_appends_suffix_only_when_truncated() {
        assert_eq!(preview("short", 10, "..."), "short");
        assert_eq!(preview("héllo world", 5, "..."), "héllo...");
    }

    #[test]
    fn score_validation_accepts_bounds_and_rejects_out_of_range() {
        assert_eq!(validate_score("knowledge", 0.0).unwrap(), 0.0);
        assert_eq!(validate_score("knowledge", 1.0).unwrap(), 1.0);
        assert!(validate_score("knowledge", -0.1).is_err());
        assert!(validate_score("knowledge", 1.5).is_err());
        assert!(validate_score("knowledge", f64::NAN).is_err());
        assert!(validate_score("knowledge", f64::INFINITY).is_err());
    }
}
