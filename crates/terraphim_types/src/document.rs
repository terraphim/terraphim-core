//! Document domain: indexed content documents and quality scoring.

use ahash::AHashMap;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::iter::IntoIterator;
use std::ops::{Deref, DerefMut};

#[cfg(feature = "typescript")]
use tsify::Tsify;

use crate::graph::Edge;
use crate::route::RouteDirective;
use crate::validation::{ValidationError, validate_score};

/// Classifies a document by its role in the knowledge graph.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentType {
    /// A knowledge graph entry (synonym / concept definition). Default variant.
    #[default]
    KgEntry,
    /// A regular content document (article, note, etc.).
    Document,
    /// A configuration document (role config, settings file, etc.).
    ConfigDocument,
}

/// Parsed directives extracted from the YAML front matter of a markdown KG entry.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkdownDirectives {
    /// How to classify this entry in the knowledge graph.
    #[serde(default)]
    pub doc_type: DocumentType,
    /// Alternative names and synonyms for the concept.
    #[serde(default)]
    pub synonyms: Vec<String>,
    /// Primary route (first in the list). Kept for backward compatibility.
    #[serde(default)]
    pub route: Option<RouteDirective>,
    /// All routes in priority order (primary first, fallbacks after).
    /// Each route may have an `action::` template for CLI invocation.
    #[serde(default)]
    pub routes: Vec<RouteDirective>,
    /// Optional display/sort priority hint (lower = higher priority).
    #[serde(default)]
    pub priority: Option<u8>,
    /// Optional trigger term that activates this entry during indexing.
    #[serde(default)]
    pub trigger: Option<String>,
    /// Whether this entry is pinned to the top of search results.
    #[serde(default)]
    pub pinned: bool,
    /// First `# Heading` from the markdown file, preserving original case.
    #[serde(default)]
    pub heading: Option<String>,
}

/// The central document type representing indexed and searchable content.
///
/// Documents are the primary unit of content in Terraphim. They can come from
/// various sources (local files, web pages, API responses) and are indexed for
/// semantic search using knowledge graphs.
///
/// # Fields
///
/// * `id` - Unique identifier (typically a UUID or URL-based ID)
/// * `url` - Source URL or file path
/// * `title` - Document title (used for display and basic search)
/// * `body` - Full text content
/// * `description` - Optional short description (extracted or provided)
/// * `summarization` - Optional AI-generated summary
/// * `stub` - Optional brief excerpt
/// * `tags` - Optional categorization tags (often from knowledge graph)
/// * `rank` - Optional relevance score from search results
/// * `source_haystack` - Optional identifier of the data source that provided this document
///
/// # Examples
///
/// ```
/// use terraphim_types::{Document, DocumentType};
///
/// let doc = Document {
///     id: "rust-book-ch1".to_string(),
///     url: "https://doc.rust-lang.org/book/ch01-00-getting-started.html".to_string(),
///     title: "Getting Started".to_string(),
///     body: "Let's start your Rust journey...".to_string(),
///     description: Some("Introduction to Rust programming".to_string()),
///     summarization: None,
///     stub: None,
///     tags: Some(vec!["rust".to_string(), "tutorial".to_string()]),
///     rank: Some(95),
///     source_haystack: Some("rust-docs".to_string()),
///     doc_type: DocumentType::KgEntry,
///     synonyms: None,
///     route: None,
///     priority: None,
///     quality_score: None,
/// };
/// ```
#[derive(Deserialize, Serialize, Debug, Clone, Default)]
#[cfg_attr(feature = "typescript", derive(Tsify))]
#[cfg_attr(feature = "typescript", tsify(into_wasm_abi, from_wasm_abi))]
pub struct Document {
    /// Unique identifier for the document
    pub id: String,
    /// URL to the document
    pub url: String,
    /// Title of the document
    pub title: String,
    /// The document body
    pub body: String,

    /// A short description of the document (extracted from content)
    pub description: Option<String>,
    /// AI-generated summarization of the document content
    pub summarization: Option<String>,
    /// A short excerpt of the document
    pub stub: Option<String>,
    /// Tags for the document
    pub tags: Option<Vec<String>>,
    /// Rank of the document in the search results
    pub rank: Option<u64>,
    /// Source haystack location that this document came from
    pub source_haystack: Option<String>,
    /// Document classification derived from directives
    #[serde(default)]
    pub doc_type: DocumentType,
    /// Synonyms extracted from directives (optional)
    #[serde(default)]
    pub synonyms: Option<Vec<String>>,
    /// Optional route directive (provider/model)
    #[serde(default)]
    pub route: Option<RouteDirective>,
    /// Optional priority directive (0-100)
    #[serde(default)]
    pub priority: Option<u8>,
    /// Quality scores for K/L/S dimensions, populated by judge system or manual review
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality_score: Option<QualityScore>,
}

impl fmt::Display for Document {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Start with title and body
        write!(f, "{} {}", self.title, self.body)?;

        // Append description if it exists
        if let Some(ref description) = self.description {
            write!(f, " {}", description)?;
        }

        // Append summarization if it exists and is different from description
        if let Some(ref summarization) = self.summarization
            && Some(summarization) != self.description.as_ref()
        {
            write!(f, " {}", summarization)?;
        }

        Ok(())
    }
}

impl Document {
    /// Set the source haystack for this document
    pub fn with_source_haystack(mut self, haystack_location: String) -> Self {
        self.source_haystack = Some(haystack_location);
        self
    }

    /// Get the source haystack location
    pub fn get_source_haystack(&self) -> Option<&String> {
        self.source_haystack.as_ref()
    }
}

/// An index is a hashmap of documents
///
/// It holds the documents that have been indexed
/// and can be searched through using the `RoleGraph`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Index {
    inner: AHashMap<String, Document>,
}

impl Default for Index {
    fn default() -> Self {
        Self::new()
    }
}

impl Index {
    /// Create a new, empty index
    pub fn new() -> Self {
        Self {
            inner: AHashMap::new(),
        }
    }

    /// Converts all given indexed documents to documents
    ///
    /// Returns the all converted documents
    pub fn get_documents(&self, docs: Vec<IndexedDocument>) -> Vec<Document> {
        let mut documents: Vec<Document> = Vec::new();
        for doc in docs {
            log::trace!("doc: {:#?}", doc);
            if let Some(document) = self.get_document(&doc) {
                // Document found in cache
                let mut document = document;
                document.tags = Some(doc.tags.clone());
                // rank only available for terraphim graph
                // use scorer to populate the rank for all cases
                document.rank = Some(doc.rank);
                document.quality_score = doc.quality_score.clone();
                documents.push(document);
            } else {
                log::warn!("Document not found in cache. Cannot convert.");
            }
        }
        documents
    }
    /// Returns all documents from the index for scorer without graph embeddings
    pub fn get_all_documents(&self) -> Vec<Document> {
        let documents: Vec<Document> = self.values().cloned().collect::<Vec<Document>>();
        documents
    }

    /// Get a document from the index (if it exists in the index)
    pub fn get_document(&self, doc: &IndexedDocument) -> Option<Document> {
        if let Some(document) = self.inner.get(&doc.id).cloned() {
            // Document found in cache
            let mut document = document;
            document.tags = Some(doc.tags.clone());
            // Rank only available for terraphim graph
            // use scorer to populate the rank for all cases
            document.rank = Some(doc.rank);
            document.quality_score = doc.quality_score.clone();
            Some(document)
        } else {
            None
        }
    }
}

impl Deref for Index {
    type Target = AHashMap<String, Document>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for Index {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl IntoIterator for Index {
    type Item = (String, Document);
    type IntoIter = std::collections::hash_map::IntoIter<String, Document>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.into_iter()
    }
}

/// Quality scores for Knowledge/Learning/Synthesis (K/L/S) dimensions.
///
/// These scores represent the quality of a document across three dimensions:
/// - Knowledge: Depth and accuracy of domain knowledge
/// - Logic: Reasoning quality and clarity
/// - Structure: Organisation of concepts and insight
///
/// All scores are optional and range from 0.0 to 1.0 when present.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct QualityScore {
    /// Knowledge quality score (0.0-1.0)
    pub knowledge: Option<f64>,
    /// Logic quality score (0.0-1.0)
    pub logic: Option<f64>,
    /// Structure quality score (0.0-1.0)
    pub structure: Option<f64>,
    /// Timestamp when the quality was last evaluated
    pub last_evaluated: Option<chrono::DateTime<chrono::Utc>>,
}

impl QualityScore {
    /// Validated constructor enforcing the documented 0.0-1.0 invariant.
    ///
    /// Construction and deserialisation must go through the same invariant
    /// check; invalid external values yield a typed [`ValidationError`]
    /// instead of silently persisting an out-of-range score.
    ///
    /// # Examples
    ///
    /// ```
    /// use terraphim_types::QualityScore;
    ///
    /// let score = QualityScore::try_new(Some(0.8), Some(0.6), None, None).unwrap();
    /// assert_eq!(score.composite(), 0.7);
    ///
    /// assert!(QualityScore::try_new(Some(1.5), None, None, None).is_err());
    /// ```
    pub fn try_new(
        knowledge: Option<f64>,
        logic: Option<f64>,
        structure: Option<f64>,
        last_evaluated: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<Self, ValidationError> {
        Ok(Self {
            knowledge: knowledge
                .map(|v| validate_score("knowledge", v))
                .transpose()?,
            logic: logic.map(|v| validate_score("logic", v)).transpose()?,
            structure: structure
                .map(|v| validate_score("structure", v))
                .transpose()?,
            last_evaluated,
        })
    }

    /// Calculate the composite score by averaging all available scores.
    ///
    /// Returns 0.0 if no scores are available.
    ///
    /// # Examples
    ///
    /// ```
    /// use terraphim_types::QualityScore;
    ///
    /// let score = QualityScore {
    ///     knowledge: Some(0.8),
    ///     logic: Some(0.6),
    ///     structure: None,
    ///     last_evaluated: None,
    /// };
    /// assert_eq!(score.composite(), 0.7); // (0.8 + 0.6) / 2
    ///
    /// let empty = QualityScore::default();
    /// assert_eq!(empty.composite(), 0.0);
    /// ```
    pub fn composite(&self) -> f64 {
        let mut sum = 0.0;
        let mut count = 0;

        if let Some(k) = self.knowledge {
            sum += k;
            count += 1;
        }
        if let Some(l) = self.logic {
            sum += l;
            count += 1;
        }
        if let Some(s) = self.structure {
            sum += s;
            count += 1;
        }

        if count == 0 { 0.0 } else { sum / count as f64 }
    }
}

/// Reference to external storage of documents
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IndexedDocument {
    /// UUID of the indexed document, matching external storage id
    pub id: String,
    /// Matched to edges
    pub matched_edges: Vec<Edge>,
    /// Graph rank (the sum of node rank, edge rank)
    /// Number of nodes and edges connected to the document
    pub rank: u64,
    /// Tags, which are nodes turned into concepts for human readability
    pub tags: Vec<String>,
    /// List of node IDs for validation of matching
    pub nodes: Vec<u64>,
    /// Quality scores for K/L/S dimensions
    #[serde(default)]
    pub quality_score: Option<QualityScore>,
}

impl IndexedDocument {
    /// Serialises this document to a JSON string.
    pub fn to_json_string(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(&self)
    }

    /// Creates a minimal `IndexedDocument` from a [`Document`], with empty edge/node lists.
    pub fn from_document(document: Document) -> Self {
        IndexedDocument {
            id: document.id,
            matched_edges: Vec::new(),
            rank: 0,
            tags: document.tags.unwrap_or_default(),
            nodes: Vec::new(),
            quality_score: None,
        }
    }
}

/// Extract the first paragraph from document body text.
///
/// Skips YAML frontmatter (content between `---` markers) and returns
/// the first non-empty line or the first paragraph.
pub fn extract_first_paragraph(body: &str) -> String {
    // Skip YAML frontmatter if present
    let content = if body.trim_start().starts_with("---") {
        // Find the end of frontmatter
        if let Some(end_pos) = body[3..].find("---") {
            &body[end_pos + 6..] // Skip past the closing ---
        } else {
            body
        }
    } else {
        body
    };

    // Find first non-empty line
    for line in content.lines() {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    // Fallback to empty string if no content found
    String::new()
}
