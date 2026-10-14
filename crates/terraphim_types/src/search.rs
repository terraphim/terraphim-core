//! Search domain: queries, logical operators, output layers and relevance functions.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[cfg(feature = "typescript")]
use tsify::Tsify;

use crate::role::RoleName;
use crate::term::NormalizedTermValue;

/// Logical operators for combining multiple search terms
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, JsonSchema)]
#[cfg_attr(feature = "typescript", derive(Tsify))]
#[cfg_attr(feature = "typescript", tsify(into_wasm_abi, from_wasm_abi))]
pub enum LogicalOperator {
    /// AND operator - documents must contain all terms
    #[serde(rename = "and")]
    And,
    /// OR operator - documents may contain any of the terms
    #[serde(rename = "or")]
    Or,
}

/// Layered output levels for search results.
///
/// Controls how much content is returned per search result to optimize token usage:
/// - Layer 1: Title + tags only (~50 tokens/result)
/// - Layer 2: + first paragraph summary (~150 tokens/result)
/// - Layer 3: Full content (current default behaviour)
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default, JsonSchema)]
#[cfg_attr(feature = "typescript", derive(Tsify))]
#[cfg_attr(feature = "typescript", tsify(into_wasm_abi, from_wasm_abi))]
pub enum Layer {
    /// Title + tags only (~50 tokens/result)
    #[serde(rename = "1")]
    #[default]
    One,
    /// + first paragraph summary (~150 tokens/result)
    #[serde(rename = "2")]
    Two,
    /// Full content (default)
    #[serde(rename = "3")]
    Three,
}

impl Layer {
    /// Parse a layer from an integer value (1, 2, or 3)
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(Layer::One),
            2 => Some(Layer::Two),
            3 => Some(Layer::Three),
            _ => None,
        }
    }

    /// Returns true if this layer includes content (layer 2 or 3)
    pub fn includes_content(&self) -> bool {
        matches!(self, Layer::Two | Layer::Three)
    }

    /// Returns true if this layer includes full content (layer 3)
    pub fn includes_full_content(&self) -> bool {
        matches!(self, Layer::Three)
    }
}

impl std::fmt::Display for Layer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Layer::One => write!(f, "1"),
            Layer::Two => write!(f, "2"),
            Layer::Three => write!(f, "3"),
        }
    }
}

/// A search query for finding documents in the knowledge graph.
///
/// Supports both single-term and multi-term queries with logical operators (AND/OR).
/// Results can be paginated using `skip` and `limit`, and scoped to specific roles.
///
/// # Examples
///
/// ## Single-term query
///
/// ```
/// use terraphim_types::{SearchQuery, NormalizedTermValue, Layer, RoleName};
///
/// let query = SearchQuery {
///     search_term: NormalizedTermValue::from("machine learning"),
///     search_terms: None,
///     operator: None,
///     skip: None,
///     limit: Some(10),
///     role: Some(RoleName::new("data_scientist")),
///     layer: Layer::default(),
///     include_pinned: false,
///     min_quality: None,
/// };
/// ```
///
/// ## Multi-term AND query
///
/// ```
/// use terraphim_types::{SearchQuery, NormalizedTermValue, LogicalOperator, RoleName};
///
/// let query = SearchQuery::with_terms_and_operator(
///     NormalizedTermValue::from("rust"),
///     vec![NormalizedTermValue::from("async"), NormalizedTermValue::from("tokio")],
///     LogicalOperator::And,
///     Some(RoleName::new("engineer")),
/// );
/// assert!(query.is_multi_term_query());
/// assert_eq!(query.get_all_terms().len(), 3);
/// ```
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[cfg_attr(feature = "typescript", derive(Tsify))]
#[cfg_attr(feature = "typescript", tsify(into_wasm_abi, from_wasm_abi))]
pub struct SearchQuery {
    /// Primary search term for backward compatibility
    #[serde(alias = "query")]
    pub search_term: NormalizedTermValue,
    /// Multiple search terms for logical operations
    pub search_terms: Option<Vec<NormalizedTermValue>>,
    /// Logical operator for combining multiple terms (defaults to OR if not specified)
    pub operator: Option<LogicalOperator>,
    /// Number of results to skip (for pagination)
    pub skip: Option<usize>,
    /// Maximum number of results to return
    pub limit: Option<usize>,
    /// Role context for this search
    pub role: Option<RoleName>,
    /// Output layer for controlling result detail (1=minimal, 2=summary, 3=full)
    #[serde(default)]
    pub layer: Layer,
    /// Include pinned KG entries in results even if they don't match the query
    #[serde(default)]
    pub include_pinned: bool,
    /// Minimum composite quality score threshold (0.0–1.0). Documents with a composite
    /// score below this value are excluded from results.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_quality: Option<f64>,
}

impl SearchQuery {
    /// Get all search terms (both single and multiple)
    pub fn get_all_terms(&self) -> Vec<&NormalizedTermValue> {
        if let Some(ref multiple_terms) = self.search_terms {
            // For multi-term queries, include primary term + additional terms,
            // but avoid duplicates when the primary term is also present in `search_terms`.
            let mut all_terms: Vec<&NormalizedTermValue> =
                Vec::with_capacity(1 + multiple_terms.len());
            all_terms.push(&self.search_term);

            for term in multiple_terms.iter() {
                if term.as_str() != self.search_term.as_str() {
                    all_terms.push(term);
                }
            }

            all_terms
        } else {
            // For single-term queries, use search_term
            vec![&self.search_term]
        }
    }

    /// Check if this is a multi-term query with logical operators
    pub fn is_multi_term_query(&self) -> bool {
        self.search_terms.is_some() && !self.search_terms.as_ref().unwrap().is_empty()
    }

    /// Get the effective logical operator (defaults to Or for multi-term queries)
    pub fn get_operator(&self) -> LogicalOperator {
        self.operator
            .as_ref()
            .unwrap_or(&LogicalOperator::Or)
            .clone()
    }

    /// Create a new SearchQuery with multiple terms and an operator
    pub fn with_terms_and_operator(
        primary_term: NormalizedTermValue,
        additional_terms: Vec<NormalizedTermValue>,
        operator: LogicalOperator,
        role: Option<RoleName>,
    ) -> Self {
        Self {
            search_term: primary_term,
            search_terms: Some(additional_terms),
            operator: Some(operator),
            skip: None,
            limit: None,
            role,
            layer: Layer::default(),
            include_pinned: false,
            min_quality: None,
        }
    }
}

/// Defines the relevance function (scorer) to be used for ranking search
/// results for the `Role`.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Copy, JsonSchema, Default)]
#[cfg_attr(feature = "typescript", derive(Tsify))]
#[cfg_attr(feature = "typescript", tsify(into_wasm_abi, from_wasm_abi))]
pub enum RelevanceFunction {
    /// Scorer for ranking search results based on the Terraphim graph
    ///
    /// This is based on filtered result outputs according to the ranking of the
    /// knowledge graph. The node, which is most connected will produce the
    /// highest ranking
    #[serde(rename = "terraphim-graph")]
    TerraphimGraph,
    /// Scorer for ranking search results based on the title of a document
    #[default]
    #[serde(rename = "title-scorer")]
    TitleScorer,
    /// BM25 (Okapi BM25) relevance function for probabilistic ranking
    #[serde(rename = "bm25")]
    BM25,
    /// BM25F relevance function with field-specific weights (title, body, description, tags)
    #[serde(rename = "bm25f")]
    BM25F,
    /// BM25Plus relevance function with enhanced parameters for fine-tuning
    #[serde(rename = "bm25plus")]
    BM25Plus,
}

/// Defines all supported inputs for the knowledge graph.
///
/// Every knowledge graph is built from a specific input, such as Markdown files
/// or JSON files.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, JsonSchema)]
#[cfg_attr(feature = "typescript", derive(Tsify))]
#[cfg_attr(feature = "typescript", tsify(into_wasm_abi, from_wasm_abi))]
pub enum KnowledgeGraphInputType {
    /// A set of Markdown files
    #[serde(rename = "markdown")]
    Markdown,
    /// A JSON files
    #[serde(rename = "json")]
    Json,
}
