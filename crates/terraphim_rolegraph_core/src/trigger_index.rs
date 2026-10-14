//! Aho-Corasick-free TF-IDF trigger index.
//!
//! Extracted from `terraphim_rolegraph::TriggerIndex` (lib.rs:76) so it
//! can be used in dep-light contexts. The algorithm and field structure
//! are byte-for-byte identical to the upstream; only the
//! `terraphim_automata` dep is dropped (and that dep was never used by
//! `TriggerIndex` itself — only by the surrounding `RoleGraph`).
//!
//! ## Algorithm
//!
//! For each `node_id -> description` pair:
//! 1. Tokenise the description (lowercase, alphanumeric, stopword-filter)
//! 2. Build per-node token set
//! 3. Compute smoothed IDF: `log((N + 1) / (df + 1)) + 1`
//!
//! At query time, compute TF-IDF cosine similarity between the query
//! tokens and each document's tokens. Return matches above the threshold.

use ahash::{AHashMap, AHashSet};
use serde::{Deserialize, Serialize};

/// Default relevance threshold. Matches upstream `terraphim_rolegraph`.
pub const DEFAULT_TRIGGER_THRESHOLD: f64 = 0.3;

/// IDF smoothing constant. Matches upstream's
/// `log((N + 1) / (df + 1)) + 1` formula.
pub const IDF_SMOOTHING: f64 = 1.0;

/// A TF-IDF trigger index over arbitrary string descriptions.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TriggerIndex {
    /// Map from `node_id` to its trigger description tokens (lowercased).
    triggers: AHashMap<u64, Vec<String>>,
    /// Inverse document frequency for each token.
    idf: AHashMap<String, f64>,
    /// Total number of documents with triggers.
    doc_count: usize,
    /// Configurable relevance threshold (0.0-1.0).
    threshold: f64,
    /// Custom stopwords (if `None`, uses the default built-in set).
    custom_stopwords: Option<AHashSet<String>>,
}

impl TriggerIndex {
    /// Create a new `TriggerIndex` with the given threshold and default
    /// stopwords. Non-finite thresholds fall back to
    /// [`DEFAULT_TRIGGER_THRESHOLD`].
    #[must_use]
    pub fn new(threshold: f64) -> Self {
        Self {
            triggers: AHashMap::new(),
            idf: AHashMap::new(),
            doc_count: 0,
            threshold: Self::sanitise_threshold(threshold),
            custom_stopwords: None,
        }
    }

    /// Returns `threshold` if finite, otherwise the default threshold.
    fn sanitise_threshold(threshold: f64) -> f64 {
        if threshold.is_finite() {
            threshold
        } else {
            DEFAULT_TRIGGER_THRESHOLD
        }
    }

    /// Create a new `TriggerIndex` with custom stopwords. The custom
    /// stopwords completely replace the default set.
    #[must_use]
    pub fn with_stopwords(threshold: f64, stopwords: AHashSet<String>) -> Self {
        Self {
            triggers: AHashMap::new(),
            idf: AHashMap::new(),
            doc_count: 0,
            threshold,
            custom_stopwords: Some(stopwords),
        }
    }

    /// Set the relevance threshold. Non-finite values are ignored.
    pub fn set_threshold(&mut self, threshold: f64) {
        self.threshold = Self::sanitise_threshold(threshold);
    }

    /// Get the current relevance threshold.
    #[must_use]
    pub fn threshold(&self) -> f64 {
        self.threshold
    }

    /// Build the index from a map of `node_id -> trigger description`.
    pub fn build(&mut self, triggers: AHashMap<u64, String>) {
        self.triggers.clear();
        self.idf.clear();
        self.doc_count = triggers.len();

        let mut doc_freq: AHashMap<String, usize> = AHashMap::new();
        for (node_id, trigger_text) in &triggers {
            let tokens = self.tokenise(trigger_text);
            let unique: AHashSet<&str> = tokens.iter().map(String::as_str).collect();
            for token in &unique {
                *doc_freq.entry((*token).to_string()).or_insert(0) += 1;
            }
            self.triggers.insert(*node_id, tokens);
        }

        let n = self.doc_count as f64;
        for (token, df) in &doc_freq {
            let idf = ((n + IDF_SMOOTHING) / (*df as f64 + IDF_SMOOTHING)).ln() + IDF_SMOOTHING;
            self.idf.insert(token.clone(), idf);
        }
    }

    /// Query the index. Returns `(node_id, score)` pairs with score above
    /// the threshold, sorted descending by score.
    #[must_use]
    pub fn query(&self, text: &str) -> Vec<(u64, f64)> {
        if self.triggers.is_empty() {
            return Vec::new();
        }

        let query_tokens = self.tokenise(text);
        if query_tokens.is_empty() {
            return Vec::new();
        }

        // Query TF-IDF vector.
        let mut query_tfidf: AHashMap<&str, f64> = AHashMap::new();
        for token in &query_tokens {
            let tf = 1.0; // binary TF
            let idf = self.idf.get(token.as_str()).copied().unwrap_or(0.0);
            *query_tfidf.entry(token.as_str()).or_insert(0.0) += tf * idf;
        }
        let query_norm: f64 = query_tfidf.values().map(|v| v * v).sum::<f64>().sqrt();
        if query_norm == 0.0 {
            return Vec::new();
        }

        let mut results: Vec<(u64, f64)> = Vec::new();
        for (node_id, trigger_tokens) in &self.triggers {
            // Document TF-IDF vector.
            let mut doc_tfidf: AHashMap<&str, f64> = AHashMap::new();
            for token in trigger_tokens {
                let tf = 1.0;
                let idf = self.idf.get(token.as_str()).copied().unwrap_or(0.0);
                *doc_tfidf.entry(token.as_str()).or_insert(0.0) += tf * idf;
            }
            let doc_norm: f64 = doc_tfidf.values().map(|v| v * v).sum::<f64>().sqrt();
            if doc_norm == 0.0 {
                continue;
            }

            // Cosine similarity.
            let dot: f64 = query_tfidf
                .iter()
                .map(|(token, q_val)| {
                    let d_val = doc_tfidf.get(token).copied().unwrap_or(0.0);
                    q_val * d_val
                })
                .sum();
            let sim = dot / (query_norm * doc_norm);
            if sim.is_finite() && sim >= self.threshold {
                results.push((*node_id, sim));
            }
        }
        // Sort descending by score with a stable node-ID tie-breaker so equal
        // scores are always returned in the same deterministic order.
        results.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });
        results
    }

    /// Returns `true` if this index has no trigger terms.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.triggers.is_empty()
    }

    /// Get the original trigger descriptions (reconstructed from tokens).
    #[must_use]
    pub fn get_trigger_descriptions(&self) -> AHashMap<u64, String> {
        self.triggers
            .iter()
            .map(|(node_id, tokens)| (*node_id, tokens.join(" ")))
            .collect()
    }

    /// Returns `true` if `word` is in the built-in default stopword list.
    #[must_use]
    pub fn is_default_stopword(word: &str) -> bool {
        DEFAULT_STOPWORDS.contains(word)
    }

    /// Tokenise text: lowercase, alphanumeric, stopword-filter, length >= 2.
    fn tokenise(&self, text: &str) -> Vec<String> {
        let stopwords: &AHashSet<String> = self
            .custom_stopwords
            .as_ref()
            .map_or(&DEFAULT_STOPWORDS, |s| s);
        let mut out = Vec::new();
        let mut current = String::new();
        for ch in text.chars() {
            if ch.is_alphanumeric() {
                current.extend(ch.to_lowercase());
            } else if !current.is_empty() {
                if current.len() >= 2 && !stopwords.contains(&current) {
                    out.push(std::mem::take(&mut current));
                } else {
                    current.clear();
                }
            }
        }
        if current.len() >= 2 && !stopwords.contains(&current) {
            out.push(current);
        }
        out
    }
}

/// Default stopwords used when no custom set is provided. Matches the
/// upstream `terraphim_rolegraph` default set.
fn default_stopwords() -> AHashSet<String> {
    let words = [
        "a", "an", "the", "and", "or", "but", "is", "are", "was", "were", "be", "been", "being",
        "have", "has", "had", "do", "does", "did", "will", "would", "could", "should", "may",
        "might", "must", "shall", "can", "this", "that", "these", "those", "i", "you", "he", "she",
        "it", "we", "they", "what", "which", "who", "when", "where", "why", "how", "all", "any",
        "both", "each", "few", "more", "most", "other", "some", "such", "no", "nor", "not", "only",
        "own", "same", "so", "than", "too", "very", "s", "t", "just", "in", "on", "at", "to",
        "for", "of", "with", "by", "from", "up", "about", "into", "through", "during", "before",
        "after", "above", "below", "between", "out", "off", "over", "under", "again", "further",
        "then", "once", "here", "there", "all", "her", "his", "their", "my", "your", "our", "its",
    ];
    words.iter().map(|s| (*s).to_string()).collect()
}

static DEFAULT_STOPWORDS: std::sync::LazyLock<AHashSet<String>> =
    std::sync::LazyLock::new(default_stopwords);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenise_lowercase_alphanumeric() {
        let idx = TriggerIndex::new(0.0);
        let toks = idx.tokenise("Hello, World! Rust-programming_2026");
        // "hello", "world", "rust", "programming" (alphanumeric, lowercase, >= 2 chars)
        // "2026" length 4, alphanumeric
        // "_" is non-alphanumeric, splits
        assert!(toks.contains(&"hello".to_string()));
        assert!(toks.contains(&"world".to_string()));
        assert!(toks.contains(&"rust".to_string()));
        assert!(toks.contains(&"programming".to_string()));
    }

    #[test]
    fn tokenise_filters_stopwords() {
        let idx = TriggerIndex::new(0.0);
        let toks = idx.tokenise("the cat and the dog");
        // "the", "and" are stopwords; "cat", "dog" remain.
        assert!(!toks.contains(&"the".to_string()));
        assert!(!toks.contains(&"and".to_string()));
        assert!(toks.contains(&"cat".to_string()));
        assert!(toks.contains(&"dog".to_string()));
    }

    #[test]
    fn query_threshold_zero_returns_all_with_positive_score() {
        let mut idx = TriggerIndex::new(0.0);
        let mut triggers: AHashMap<u64, String> = AHashMap::new();
        triggers.insert(1, "completely different topic".into());
        triggers.insert(2, "rust programming".into());
        idx.build(triggers);
        let results = idx.query("rust");
        assert!(!results.is_empty());
        // Document 2 shares "rust" with the query, so it must be present.
        let ids: Vec<u64> = results.iter().map(|(id, _)| *id).collect();
        assert!(ids.contains(&2));
    }
}

#[cfg(test)]
mod idempotency_tests {
    use super::*;

    fn index_with(threshold: f64, docs: &[(u64, &str)]) -> TriggerIndex {
        let mut idx = TriggerIndex::new(threshold);
        let triggers: AHashMap<u64, String> = docs
            .iter()
            .map(|(id, text)| (*id, (*text).to_string()))
            .collect();
        idx.build(triggers);
        idx
    }

    #[test]
    fn rebuild_with_identical_input_is_idempotent() {
        let docs = [
            (1u64, "managing dependencies in rust"),
            (2u64, "rust projects"),
        ];
        let a = index_with(0.3, &docs);
        let mut b = TriggerIndex::new(0.3);
        b.build(docs.iter().map(|(i, t)| (*i, t.to_string())).collect());
        // Rebuilding again on the same index must also converge.
        b.build(docs.iter().map(|(i, t)| (*i, t.to_string())).collect());
        assert_eq!(a.query("rust dependencies"), b.query("rust dependencies"));
    }

    #[test]
    fn equal_scores_tie_break_on_node_id() {
        let idx = index_with(0.0, &[(30, "alpha"), (10, "alpha"), (20, "alpha")]);
        let results = idx.query("alpha");
        let ids: Vec<u64> = results.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, vec![10, 20, 30]);
    }

    #[test]
    fn non_finite_thresholds_fall_back_to_default() {
        let idx = TriggerIndex::new(f64::NAN);
        assert_eq!(idx.threshold(), DEFAULT_TRIGGER_THRESHOLD);
        let idx = TriggerIndex::new(f64::INFINITY);
        assert_eq!(idx.threshold(), DEFAULT_TRIGGER_THRESHOLD);
    }

    #[test]
    fn scores_are_always_finite() {
        let idx = index_with(0.0, &[(1u64, "alpha beta"), (2u64, "gamma delta")]);
        for (_, score) in idx.query("alpha") {
            assert!(score.is_finite());
        }
    }
}
