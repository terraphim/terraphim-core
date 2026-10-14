//! Generic IS-A hierarchy embeddings, dep-light.
//!
//! Extracted from `terraphim_rolegraph::symbolic_embeddings` (lib.rs:43)
//! and decoupled from the medical-specific `MedicalNodeType`. The
//! dep-light version supports any domain by making the semantic type
//! a `String` (or a generic parameter) rather than a typed enum.
//!
//! ## Algorithm
//!
//! A `GenericSymbolicEmbedding` represents a node's position in an IS-A
//! hierarchy via its ancestor and descendant sets. Similarity is computed
//! using Jaccard similarity over the union of those sets:
//!
//! ```text
//! sim(a, b) = |A(a) ∪ D(a) ∩ A(b) ∪ D(b)| / |A(a) ∪ D(a) ∪ A(b) ∪ D(b)|
//! ```
//!
//! Two isolated nodes (empty sets) have `sim = 1.0` per the upstream
//! convention (maximally similar in the absence of hierarchy information).

use ahash::{AHashMap, AHashSet};
use serde::{Deserialize, Serialize};

/// A symbolic embedding for a single node in any IS-A hierarchy.
///
/// Generic over the semantic-type representation. The dep-light
/// re-extraction from `terraphim_rolegraph::SymbolicEmbedding` drops
/// the medical-specific `MedicalNodeType` enum and replaces it with
/// `String`. Downstream crates that need a typed semantic-type enum
/// can use the `medical` feature in `terraphim_rolegraph` instead.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GenericSymbolicEmbedding {
    /// The node ID this embedding represents.
    pub node_id: u64,
    /// Set of transitive ancestor node IDs (via IS-A parents).
    pub ancestors: AHashSet<u64>,
    /// Set of transitive descendant node IDs (via IS-A children).
    pub descendants: AHashSet<u64>,
    /// Depth in the IS-A hierarchy (0 for root nodes).
    pub depth: usize,
    /// Free-form semantic type label (e.g. `"concept"`, `"role"`,
    /// `"tool"`). The original medical crate used a typed
    /// `MedicalNodeType`; the generic version uses `String`.
    #[serde(default)]
    pub semantic_type: String,
}

impl GenericSymbolicEmbedding {
    /// Create a new generic symbolic embedding. `semantic_type` defaults
    /// to empty string if not provided.
    #[must_use]
    pub fn new(
        node_id: u64,
        ancestors: AHashSet<u64>,
        descendants: AHashSet<u64>,
        depth: usize,
    ) -> Self {
        Self {
            node_id,
            ancestors,
            descendants,
            depth,
            semantic_type: String::new(),
        }
    }

    /// Create a new generic symbolic embedding with a semantic type label.
    #[must_use]
    pub fn with_semantic_type(
        node_id: u64,
        ancestors: AHashSet<u64>,
        descendants: AHashSet<u64>,
        depth: usize,
        semantic_type: impl Into<String>,
    ) -> Self {
        Self {
            node_id,
            ancestors,
            descendants,
            depth,
            semantic_type: semantic_type.into(),
        }
    }

    /// Returns `true` if this node has no ancestors (root of the hierarchy).
    #[must_use]
    pub fn is_root(&self) -> bool {
        self.ancestors.is_empty()
    }

    /// Returns `true` if this node has no descendants (leaf of the hierarchy).
    #[must_use]
    pub fn is_leaf(&self) -> bool {
        self.descendants.is_empty()
    }

    /// Compute Jaccard similarity between this embedding and another.
    ///
    /// The Jaccard similarity is computed over the union of ancestor and
    /// descendant sets: `|A(a) ∪ D(a) ∩ A(b) ∪ D(b)| / |A(a) ∪ D(a) ∪ A(b) ∪ D(b)|`.
    /// Returns `1.0` if both sets are empty (two isolated nodes are
    /// maximally similar in the absence of hierarchy information).
    #[must_use]
    pub fn jaccard_similarity(&self, other: &GenericSymbolicEmbedding) -> f64 {
        let self_set: AHashSet<u64> = self
            .ancestors
            .iter()
            .chain(self.descendants.iter())
            .copied()
            .collect();
        let other_set: AHashSet<u64> = other
            .ancestors
            .iter()
            .chain(other.descendants.iter())
            .copied()
            .collect();

        let intersection = self_set.intersection(&other_set).count();
        let union = self_set.union(&other_set).count();

        if union == 0 {
            1.0
        } else {
            intersection as f64 / union as f64
        }
    }
}

/// A `HashMap` keyed wrapper for symbolic embeddings, with bulk
/// Jaccard-based similarity queries. Mirrors the upstream
/// `SymbolicEmbeddingIndex` API but without the medical-type coupling.
#[derive(Debug, Default, Clone)]
pub struct SymbolicEmbeddingIndex {
    embeddings: AHashMap<u64, GenericSymbolicEmbedding>,
}

impl SymbolicEmbeddingIndex {
    /// Create an empty index.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or replace an embedding.
    pub fn insert(&mut self, embedding: GenericSymbolicEmbedding) {
        self.embeddings.insert(embedding.node_id, embedding);
    }

    /// Get an embedding by `node_id`.
    #[must_use]
    pub fn get(&self, node_id: u64) -> Option<&GenericSymbolicEmbedding> {
        self.embeddings.get(&node_id)
    }

    /// Number of embeddings in the index.
    #[must_use]
    pub fn len(&self) -> usize {
        self.embeddings.len()
    }

    /// Returns `true` if the index has no embeddings.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.embeddings.is_empty()
    }

    /// Iterate over all embeddings.
    pub fn iter(&self) -> impl Iterator<Item = &GenericSymbolicEmbedding> {
        self.embeddings.values()
    }

    /// Compute Jaccard similarity between a query embedding and every
    /// embedding in the index. Returns `(node_id, score)` pairs sorted
    /// descending by score, with scores strictly greater than 0 (i.e. at
    /// least one shared ancestor or descendant).
    #[must_use]
    pub fn query_by_embedding(&self, query: &GenericSymbolicEmbedding) -> Vec<(u64, f64)> {
        let mut results: Vec<(u64, f64)> = self
            .embeddings
            .values()
            .map(|e| (e.node_id, query.jaccard_similarity(e)))
            .filter(|(_, s)| *s > 0.0)
            .collect();
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_embedding(node_id: u64, ancestors: &[u64], depth: usize) -> GenericSymbolicEmbedding {
        let ancestor_set: AHashSet<u64> = ancestors.iter().copied().collect();
        GenericSymbolicEmbedding::new(node_id, ancestor_set, AHashSet::new(), depth)
    }

    #[test]
    fn jaccard_empty_sets_returns_one() {
        let a = GenericSymbolicEmbedding::new(0, AHashSet::new(), AHashSet::new(), 0);
        let b = GenericSymbolicEmbedding::new(1, AHashSet::new(), AHashSet::new(), 0);
        assert!((a.jaccard_similarity(&b) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn jaccard_disjoint_sets_returns_zero() {
        let mut a_anc: AHashSet<u64> = AHashSet::new();
        a_anc.insert(1);
        let a = GenericSymbolicEmbedding::new(0, a_anc, AHashSet::new(), 1);
        let mut b_anc: AHashSet<u64> = AHashSet::new();
        b_anc.insert(2);
        let b = GenericSymbolicEmbedding::new(1, b_anc, AHashSet::new(), 1);
        assert!(a.jaccard_similarity(&b).abs() < f64::EPSILON);
    }

    #[test]
    fn jaccard_subset() {
        // a is {1, 2}, b is {1, 2, 3} — b is a superset of a. Jaccard = 2/3.
        let a = make_embedding(0, &[1, 2], 2);
        let b = make_embedding(1, &[1, 2, 3], 3);
        let sim = a.jaccard_similarity(&b);
        assert!((sim - 2.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn index_query_returns_sorted_results() {
        let mut idx = SymbolicEmbeddingIndex::new();
        idx.insert(make_embedding(1, &[10, 11], 2));
        idx.insert(make_embedding(2, &[10], 1));
        idx.insert(make_embedding(3, &[20], 1));
        let query = make_embedding(99, &[10, 11], 2);
        let results = idx.query_by_embedding(&query);
        // Node 1 shares both {10, 11} with query (Jaccard = 1.0)
        // Node 2 shares {10} (Jaccard = 1/3 = 0.333)
        // Node 3 shares nothing (filtered out)
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, 1);
        assert!((results[0].1 - 1.0).abs() < f64::EPSILON);
        assert_eq!(results[1].0, 2);
    }
}
