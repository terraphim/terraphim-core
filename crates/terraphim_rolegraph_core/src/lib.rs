//! Dep-light core types extracted from `terraphim_rolegraph`.
//!
//! This crate provides two reusable primitives for knowledge-graph retrieval
//! without pulling in `terraphim_automata` (which transitively depends on
//! `tokio` + `reqwest` + `bincode`):
//!
//! - [`TriggerIndex`] — Aho-Corasick-free TF-IDF over arbitrary string
//!   descriptions. Used for relevance scoring of trigger terms against a
//!   query.
//! - [`GenericSymbolicEmbedding`] — generic IS-A hierarchy embedding
//!   (ancestor/descendant sets + depth). Supports Jaccard similarity for
//!   any graph domain, not just medical.
//!
//! ## Origin
//!
//! Extracted from `terraphim_rolegraph` per `terraphim-core#16`. The medical
//! specialization (`SymbolicEmbedding` with `MedicalNodeType`) remains in
//! the parent crate; the generic version here serves dep-light consumers
//! like `agent-communication-protocol`.
//!
//! ## What is NOT here
//!
//! - `RoleGraph`, `RoleGraphSync` — these remain in `terraphim_rolegraph`
//!   because they carry the `AhoCorasick` + thesaurus loaders.
//! - `terraphim_automata` dep — explicitly excluded to keep this crate
//!   dep-light.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod symbolic_embedding;
mod trigger_index;

pub use symbolic_embedding::{GenericSymbolicEmbedding, SymbolicEmbeddingIndex};
pub use trigger_index::{DEFAULT_TRIGGER_THRESHOLD, TriggerIndex};

/// Default IDF smoothing constant for [`TriggerIndex`]. Matches the upstream
/// `terraphim_rolegraph` constant.
pub use trigger_index::IDF_SMOOTHING;

#[cfg(test)]
mod tests {
    use super::*;
    use ahash::AHashMap;

    /// Round-trip a SymbolicEmbedding through serde without a medical type.
    #[test]
    fn generic_embedding_round_trip() {
        use ahash::AHashSet;
        let mut ancestors: AHashSet<u64> = AHashSet::new();
        ancestors.insert(1);
        ancestors.insert(2);
        let embedding = GenericSymbolicEmbedding::new(42, ancestors, AHashSet::new(), 2);
        let json = serde_json::to_string(&embedding).unwrap();
        let back: GenericSymbolicEmbedding = serde_json::from_str(&json).unwrap();
        assert_eq!(embedding.node_id, back.node_id);
        assert_eq!(embedding.depth, back.depth);
    }

    /// Jaccard similarity is symmetric and self-similarity = 1.0.
    #[test]
    fn jaccard_symmetric() {
        use ahash::AHashSet;
        let mut a_ancestors: AHashSet<u64> = AHashSet::new();
        a_ancestors.insert(1);
        a_ancestors.insert(2);
        let a = GenericSymbolicEmbedding::new(0, a_ancestors.clone(), AHashSet::new(), 1);

        let mut b_ancestors: AHashSet<u64> = AHashSet::new();
        b_ancestors.insert(2);
        b_ancestors.insert(3);
        let b = GenericSymbolicEmbedding::new(1, b_ancestors, AHashSet::new(), 1);

        let sim_ab = a.jaccard_similarity(&b);
        let sim_ba = b.jaccard_similarity(&a);
        assert!((sim_ab - sim_ba).abs() < f64::EPSILON);
        // a and b share {2}, total {1,2,3} = 3. Jaccard = 1/3.
        assert!((sim_ab - 1.0 / 3.0).abs() < 1e-9);
    }

    /// Self-similarity is 1.0 (empty union fallback).
    #[test]
    fn jaccard_self_is_one() {
        use ahash::AHashSet;
        let embedding = GenericSymbolicEmbedding::new(0, AHashSet::new(), AHashSet::new(), 0);
        assert!((embedding.jaccard_similarity(&embedding) - 1.0).abs() < f64::EPSILON);
    }

    /// `TriggerIndex::query` returns sorted descending by score, above
    /// threshold.
    #[test]
    fn trigger_query_returns_sorted_results() {
        let mut idx = TriggerIndex::new(0.0);
        let mut triggers: AHashMap<u64, String> = AHashMap::new();
        triggers.insert(1, "rust programming language".into());
        triggers.insert(2, "rust async runtime".into());
        triggers.insert(3, "python interpreter".into());
        idx.build(triggers);
        let results = idx.query("rust");
        assert!(!results.is_empty());
        // Rust-related docs (1, 2) should rank above Python doc (3).
        let scores: Vec<f64> = results.iter().map(|(_, s)| *s).collect();
        for w in scores.windows(2) {
            assert!(
                w[0] >= w[1],
                "results must be sorted descending: {scores:?}"
            );
        }
    }

    /// `TriggerIndex` with no triggers returns empty results.
    #[test]
    fn trigger_empty_returns_empty() {
        let idx = TriggerIndex::new(0.0);
        assert!(idx.query("anything").is_empty());
    }
}
