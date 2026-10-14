//! Ontology domain: schema-first knowledge graph definitions and grounding metadata.

use serde::{Deserialize, Serialize};
// ============================================================================
// Dynamic Ontology Types - Schema-First Knowledge Graph with Grounding
// ============================================================================

/// Normalization method used for grounding
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum NormalizationMethod {
    /// Exact match via Aho-Corasick
    #[default]
    Exact,
    /// Fuzzy match via Levenshtein or Jaro-Winkler
    Fuzzy,
    /// Graph rank-based prioritization
    GraphRank,
}

/// Grounding metadata for normalized terms (Dynamic Ontology)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GroundingMetadata {
    /// Canonical URI from ontology (NCIt, HGNC, etc.)
    pub normalized_uri: Option<String>,
    /// Human-friendly label for display
    pub normalized_label: Option<String>,
    /// Source ontology (NCIt, HGNC, custom)
    pub normalized_prov: Option<String>,
    /// Similarity/confidence score (0.0 - 1.0)
    pub normalized_score: Option<f32>,
    /// Method used for normalization
    pub normalized_method: Option<NormalizationMethod>,
}

impl GroundingMetadata {
    /// Create new grounding metadata with URI and score
    pub fn new(
        uri: String,
        label: String,
        prov: String,
        score: f32,
        method: NormalizationMethod,
    ) -> Self {
        Self {
            normalized_uri: Some(uri),
            normalized_label: Some(label),
            normalized_prov: Some(prov),
            normalized_score: Some(score),
            normalized_method: Some(method),
        }
    }
}

/// Coverage governance signal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageSignal {
    /// Total categories in extracted schema
    pub total_categories: usize,
    /// Categories matched in ontology catalog
    pub matched_categories: usize,
    /// Coverage ratio = matched/total
    pub coverage_ratio: f32,
    /// Threshold for needing review
    pub threshold: f32,
    /// Whether this needs human review
    pub needs_review: bool,
}

impl CoverageSignal {
    /// Compute coverage signal from categories and matched count
    pub fn compute(categories: &[String], matched: usize, threshold: f32) -> Self {
        let total = categories.len();
        let ratio = if total > 0 {
            matched as f32 / total as f32
        } else {
            0.0
        };
        Self {
            total_categories: total,
            matched_categories: matched,
            coverage_ratio: ratio,
            threshold,
            needs_review: ratio < threshold,
        }
    }
}

/// Entity types for oncology schema (feature-gated)
#[cfg(feature = "medical")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityType {
    CancerDiagnosis,
    Tumor,
    GenomicVariant,
    Biomarker,
    Drug,
    Treatment,
    SideEffect,
}

/// Relationship types for oncology schema (feature-gated)
#[cfg(feature = "medical")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RelationshipType {
    HasTumor,
    HasVariant,
    HasBiomarker,
    TreatedWith,
    Causes,
    HasDiagnosis,
}

/// Extracted entity from text
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedEntity {
    /// Type of entity (string for generic cross-domain use)
    pub entity_type: String,
    /// Raw value from text
    pub raw_value: String,
    /// Normalized value if available
    pub normalized_value: Option<String>,
    /// Grounding metadata
    pub grounding: Option<GroundingMetadata>,
}

/// Extracted relationship from text
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedRelationship {
    /// Type of relationship (string for generic cross-domain use)
    pub relationship_type: String,
    /// Source entity
    pub source: String,
    /// Target entity
    pub target: String,
    /// Confidence score
    pub confidence: f32,
}

/// Schema signal extracted from text
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaSignal {
    /// Extracted entities
    pub entities: Vec<ExtractedEntity>,
    /// Extracted relationships
    pub relationships: Vec<ExtractedRelationship>,
    /// Overall confidence score
    pub confidence: f32,
}

// ============================================================================
// Ontology Schema Types - Schema-First Knowledge Graph Definition (#547)
// ============================================================================

/// Entity type definition in an ontology schema
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyEntityType {
    /// Unique identifier within the schema (e.g., "chapter", "concept", "author")
    pub id: String,
    /// Human-readable label
    pub label: String,
    /// Canonical URI prefix for grounding (e.g., `https://schema.org/Chapter`)
    #[serde(default)]
    pub uri_prefix: Option<String>,
    /// Alternative names / synonyms for matching
    #[serde(default)]
    pub aliases: Vec<String>,
    /// Category for coverage grouping (e.g., "core", "supporting", "optional")
    #[serde(default)]
    pub category: Option<String>,
}

/// Relationship type definition in an ontology schema
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyRelationshipType {
    /// Relationship identifier (e.g., "references", "defines")
    pub id: String,
    /// Human-readable label
    pub label: String,
    /// Source entity type ID
    pub source_type: String,
    /// Target entity type ID
    pub target_type: String,
}

/// Anti-pattern definition for detection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyAntiPattern {
    /// Anti-pattern identifier
    pub id: String,
    /// Description of what this anti-pattern represents
    pub description: String,
    /// Terms that indicate this anti-pattern
    pub indicators: Vec<String>,
}

/// Schema-first ontology definition
///
/// Loaded from JSON file, used to build thesaurus for extraction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologySchema {
    /// Schema name
    pub name: String,
    /// Schema version
    pub version: String,
    /// Entity type definitions
    pub entity_types: Vec<OntologyEntityType>,
    /// Relationship type definitions
    #[serde(default)]
    pub relationship_types: Vec<OntologyRelationshipType>,
    /// Anti-patterns to detect
    #[serde(default)]
    pub anti_patterns: Vec<OntologyAntiPattern>,
}

impl OntologySchema {
    /// Load schema from JSON file
    pub fn load_from_file(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let schema: Self = serde_json::from_str(&content)?;
        Ok(schema)
    }

    /// Build thesaurus entries from schema entity types + aliases
    ///
    /// Each entity type label and its aliases become thesaurus entries
    /// with the URI prefix as the URL for grounding.
    /// Returns tuples of (id, term, url).
    pub fn to_thesaurus_entries(&self) -> Vec<(String, String, Option<String>)> {
        let mut entries = Vec::new();
        for entity_type in &self.entity_types {
            let url = entity_type
                .uri_prefix
                .clone()
                .unwrap_or_else(|| format!("kg://{}", entity_type.id));
            // Primary label
            entries.push((
                entity_type.id.clone(),
                entity_type.label.clone(),
                Some(url.clone()),
            ));
            // Aliases
            for alias in &entity_type.aliases {
                entries.push((entity_type.id.clone(), alias.clone(), Some(url.clone())));
            }
        }
        entries
    }

    /// Get all entity type IDs for coverage calculation
    pub fn category_ids(&self) -> Vec<String> {
        self.entity_types.iter().map(|e| e.id.clone()).collect()
    }

    /// Get URI for a matched entity type ID
    pub fn uri_for(&self, entity_type_id: &str) -> Option<String> {
        self.entity_types
            .iter()
            .find(|e| e.id == entity_type_id)
            .and_then(|e| e.uri_prefix.clone())
    }
}
