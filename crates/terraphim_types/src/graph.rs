//! Graph domain: knowledge graph nodes, edges and thesauri.

use ahash::AHashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::collections::hash_map::Iter;
use std::iter::IntoIterator;

#[cfg(feature = "medical")]
use crate::medical_types;
use crate::term::{NormalizedTerm, NormalizedTermValue};

/// A directed relationship between two nodes in the knowledge graph.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Edge {
    /// ID of the edge (u64)
    pub id: u64,
    /// Rank of the edge
    pub rank: u64,
    /// A hashmap of `document_id` to `rank`
    pub doc_hash: AHashMap<String, u64>,
    /// Medical edge type (only available with the `medical` feature)
    #[cfg(feature = "medical")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge_type: Option<medical_types::MedicalEdgeType>,
}

impl Edge {
    /// Creates an edge with the given ID pointing to `document_id` with an initial rank of 1.
    pub fn new(id: u64, document_id: String) -> Self {
        let mut doc_hash = AHashMap::new();
        doc_hash.insert(document_id, 1);
        Self {
            id,
            rank: 1,
            doc_hash,
            #[cfg(feature = "medical")]
            edge_type: None,
        }
    }
}

/// A `Node` represents single concept and its connections to other concepts.
///
/// Each node can have multiple edges to other nodes
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Node {
    /// Unique identifier of the node (u64)
    pub id: u64,
    /// Number of co-occurrences
    pub rank: u64,
    /// List of connected edges
    pub connected_with: HashSet<u64>,
    /// Medical node type (only available with the `medical` feature)
    #[cfg(feature = "medical")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_type: Option<medical_types::MedicalNodeType>,
    /// Human-readable term for this node (only available with the `medical` feature)
    #[cfg(feature = "medical")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub term: Option<String>,
    /// SNOMED CT concept identifier (only available with the `medical` feature)
    #[cfg(feature = "medical")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snomed_id: Option<u64>,
}

impl Node {
    /// Create a new node with a given id and edge
    pub fn new(id: u64, edge: Edge) -> Self {
        let mut connected_with = HashSet::new();
        connected_with.insert(edge.id);
        Self {
            id,
            rank: 1,
            connected_with,
            #[cfg(feature = "medical")]
            node_type: None,
            #[cfg(feature = "medical")]
            term: None,
            #[cfg(feature = "medical")]
            snomed_id: None,
        }
    }

    // pub fn sort_edges_by_value(&self) {
    //     // let count_b: BTreeMap<&u64, &Edge> =
    //     // self.connected_with.iter().map(|(k, v)| (v, k)).collect();
    //     // for (k, v) in self.connected_with.iter().map(|(k, v)| (v.rank, k)) {
    //     // log::warn!("k {:?} v {:?}", k, v);
    //     // }
    //     log::warn!("Connected with {:?}", self.connected_with);
    // }
}

/// A thesaurus is a dictionary with synonyms which map to upper-level concepts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Thesaurus {
    /// Name of the thesaurus
    name: String,
    /// The inner hashmap of normalized terms
    data: AHashMap<NormalizedTermValue, NormalizedTerm>,
    /// SHA-256 hash of the source markdown files used to build this thesaurus.
    /// Used for cache invalidation: when the hash changes, the thesaurus is rebuilt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_hash: Option<String>,
}

impl Thesaurus {
    /// Create a new, empty thesaurus
    pub fn new(name: String) -> Self {
        Self {
            name,
            data: AHashMap::new(),
            source_hash: None,
        }
    }

    /// Set the source hash for cache invalidation tracking.
    pub fn with_source_hash(mut self, hash: String) -> Self {
        self.source_hash = Some(hash);
        self
    }

    /// Get the name of the thesaurus
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Inserts a key-value pair into the thesaurus.
    pub fn insert(&mut self, key: NormalizedTermValue, value: NormalizedTerm) {
        self.data.insert(key, value);
    }

    /// Get the length of the thesaurus
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Check if the thesaurus is empty
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Custom `get` method for the thesaurus, which accepts a
    /// `NormalizedTermValue` and returns a reference to the
    /// `NormalizedTerm`.
    pub fn get(&self, key: &NormalizedTermValue) -> Option<&NormalizedTerm> {
        self.data.get(key)
    }

    /// Returns an iterator over all normalised term keys in the thesaurus.
    pub fn keys(
        &self,
    ) -> std::collections::hash_map::Keys<'_, NormalizedTermValue, NormalizedTerm> {
        self.data.keys()
    }
}

// Implement `IntoIterator` for a reference to `Thesaurus`
impl<'a> IntoIterator for &'a Thesaurus {
    type Item = (&'a NormalizedTermValue, &'a NormalizedTerm);
    type IntoIter = Iter<'a, NormalizedTermValue, NormalizedTerm>;

    fn into_iter(self) -> Self::IntoIter {
        self.data.iter()
    }
}
