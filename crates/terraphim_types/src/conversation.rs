//! Conversation domain: LLM conversations, context items and usage history.

use ahash::AHashMap;
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display, Formatter};

#[cfg(feature = "typescript")]
use tsify::Tsify;

use crate::document::Document;

use crate::role::RoleName;
use crate::term::NormalizedTermValue;
use crate::validation::preview;

// Context Management Types for LLM Conversations

/// Unique identifier for conversations
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(Tsify))]
#[cfg_attr(feature = "typescript", tsify(into_wasm_abi, from_wasm_abi))]
pub struct ConversationId(pub String);

impl ConversationId {
    /// Generates a new random UUID-based conversation ID.
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    /// Wraps an existing string as a conversation ID.
    pub fn from_string(id: String) -> Self {
        Self(id)
    }

    /// Returns the ID as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for ConversationId {
    fn default() -> Self {
        Self::new()
    }
}

impl Display for ConversationId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Types of context that can be added to conversations
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(Tsify))]
#[cfg_attr(feature = "typescript", tsify(into_wasm_abi, from_wasm_abi))]
pub enum ContextType {
    /// System-level context
    System,
    /// User-provided context
    UserInput,
    /// Document-based context
    Document,
    /// Search result context
    SearchResult,
    /// External data or API context
    External,
    /// Context from KG term definition with synonyms and metadata
    KGTermDefinition,
    /// Context from complete knowledge graph index
    KGIndex,
}

/// Unique identifier for messages within conversations
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(Tsify))]
#[cfg_attr(feature = "typescript", tsify(into_wasm_abi, from_wasm_abi))]
pub struct MessageId(pub String);

impl MessageId {
    /// Generates a new random UUID-based message ID.
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    /// Wraps an existing string as a message ID.
    pub fn from_string(id: String) -> Self {
        Self(id)
    }

    /// Returns the ID as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for MessageId {
    fn default() -> Self {
        Self::new()
    }
}

impl Display for MessageId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Context item that can be added to LLM conversations
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(Tsify))]
#[cfg_attr(feature = "typescript", tsify(into_wasm_abi, from_wasm_abi))]
pub struct ContextItem {
    /// Unique identifier for the context item
    pub id: String,
    /// Type of context (document, search_result, user_input, etc.)
    pub context_type: ContextType,
    /// Title or summary of the context item
    pub title: String,
    /// Brief summary of the content (separate from full content)
    pub summary: Option<String>,
    /// The actual content to be included in the LLM context
    pub content: String,
    /// Metadata about the context (source, relevance score, etc.)
    pub metadata: AHashMap<String, String>,
    /// Timestamp when this context was added
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Relevance score for ordering context items
    pub relevance_score: Option<f64>,
}

impl ContextItem {
    /// Create a new context item from a document
    pub fn from_document(document: &Document) -> Self {
        let mut metadata = AHashMap::new();
        metadata.insert("source_type".to_string(), "document".to_string());
        metadata.insert("document_id".to_string(), document.id.clone());
        if !document.url.is_empty() {
            metadata.insert("url".to_string(), document.url.clone());
        }
        if let Some(tags) = &document.tags {
            metadata.insert("tags".to_string(), tags.join(", "));
        }
        if let Some(rank) = document.rank {
            metadata.insert("rank".to_string(), rank.to_string());
        }

        Self {
            id: uuid::Uuid::new_v4().to_string(),
            context_type: ContextType::Document,
            title: if document.title.is_empty() {
                document.id.clone()
            } else {
                document.title.clone()
            },
            summary: document.description.clone(),
            content: format!(
                "Title: {}\n\n{}\n\n{}",
                document.title,
                document.description.as_deref().unwrap_or(""),
                document.body
            ),
            metadata,
            created_at: chrono::Utc::now(),
            relevance_score: document.rank.map(|r| r as f64),
        }
    }

    /// Create a new context item from search results
    pub fn from_search_result(query: &str, documents: &[Document]) -> Self {
        let mut metadata = AHashMap::new();
        metadata.insert("source_type".to_string(), "search_result".to_string());
        metadata.insert("query".to_string(), query.to_string());
        metadata.insert("result_count".to_string(), documents.len().to_string());

        let content = if documents.is_empty() {
            format!("Search query: '{}'\nNo results found.", query)
        } else {
            let mut content = format!("Search query: '{}'\nResults:\n\n", query);
            for (i, doc) in documents.iter().take(5).enumerate() {
                content.push_str(&format!(
                    "{}. {}\n   {}\n   Rank: {}\n\n",
                    i + 1,
                    doc.title,
                    doc.description.as_deref().unwrap_or("No description"),
                    doc.rank.unwrap_or(0)
                ));
            }
            if documents.len() > 5 {
                content.push_str(&format!("... and {} more results\n", documents.len() - 5));
            }
            content
        };

        Self {
            id: uuid::Uuid::new_v4().to_string(),
            context_type: ContextType::Document, // Changed from SearchResult to Document
            title: format!("Search: {}", query),
            summary: Some(format!(
                "Search results for '{}' - {} documents found",
                query,
                documents.len()
            )),
            content,
            metadata,
            created_at: chrono::Utc::now(),
            relevance_score: documents.first().and_then(|d| d.rank.map(|r| r as f64)),
        }
    }

    /// Create a new context item from a KG term definition
    pub fn from_kg_term_definition(kg_term: &KGTermDefinition) -> Self {
        let mut metadata = AHashMap::new();
        metadata.insert("source_type".to_string(), "kg_term".to_string());
        metadata.insert("term_id".to_string(), kg_term.id.to_string());
        metadata.insert(
            "normalized_term".to_string(),
            kg_term.normalized_term.to_string(),
        );
        metadata.insert(
            "synonyms_count".to_string(),
            kg_term.synonyms.len().to_string(),
        );
        metadata.insert(
            "related_terms_count".to_string(),
            kg_term.related_terms.len().to_string(),
        );
        metadata.insert(
            "usage_examples_count".to_string(),
            kg_term.usage_examples.len().to_string(),
        );

        if let Some(ref url) = kg_term.url {
            metadata.insert("url".to_string(), url.clone());
        }

        // Add KG-specific metadata
        for (key, value) in &kg_term.metadata {
            metadata.insert(format!("kg_{}", key), value.clone());
        }

        let mut content = format!("**Term:** {}\n", kg_term.term);

        if let Some(ref definition) = kg_term.definition {
            content.push_str(&format!("**Definition:** {}\n", definition));
        }

        if !kg_term.synonyms.is_empty() {
            content.push_str(&format!("**Synonyms:** {}\n", kg_term.synonyms.join(", ")));
        }

        if !kg_term.related_terms.is_empty() {
            content.push_str(&format!(
                "**Related Terms:** {}\n",
                kg_term.related_terms.join(", ")
            ));
        }

        if !kg_term.usage_examples.is_empty() {
            content.push_str("**Usage Examples:**\n");
            for (i, example) in kg_term.usage_examples.iter().enumerate() {
                content.push_str(&format!("{}. {}\n", i + 1, example));
            }
        }

        Self {
            id: uuid::Uuid::new_v4().to_string(),
            context_type: ContextType::KGTermDefinition,
            title: format!("KG Term: {}", kg_term.term),
            summary: Some(format!(
                "Knowledge Graph term '{}' with {} synonyms and {} related terms",
                kg_term.term,
                kg_term.synonyms.len(),
                kg_term.related_terms.len()
            )),
            content,
            metadata,
            created_at: chrono::Utc::now(),
            relevance_score: kg_term.relevance_score,
        }
    }

    /// Create a new context item from a complete KG index
    pub fn from_kg_index(kg_index: &KGIndexInfo) -> Self {
        let mut metadata = AHashMap::new();
        metadata.insert("source_type".to_string(), "kg_index".to_string());
        metadata.insert("kg_name".to_string(), kg_index.name.clone());
        metadata.insert("total_terms".to_string(), kg_index.total_terms.to_string());
        metadata.insert("total_nodes".to_string(), kg_index.total_nodes.to_string());
        metadata.insert("total_edges".to_string(), kg_index.total_edges.to_string());
        metadata.insert("source".to_string(), kg_index.source.clone());
        metadata.insert(
            "last_updated".to_string(),
            kg_index.last_updated.to_rfc3339(),
        );

        if let Some(ref version) = kg_index.version {
            metadata.insert("version".to_string(), version.clone());
        }

        let content = format!(
            "**Knowledge Graph Index: {}**\n\n\
            **Statistics:**\n\
            - Total Terms: {}\n\
            - Total Nodes: {}\n\
            - Total Edges: {}\n\
            - Source: {}\n\
            - Last Updated: {}\n\
            - Version: {}\n\n\
            This context includes the complete knowledge graph index with all terms, \
            relationships, and metadata available for reference.",
            kg_index.name,
            kg_index.total_terms,
            kg_index.total_nodes,
            kg_index.total_edges,
            kg_index.source,
            kg_index.last_updated.format("%Y-%m-%d %H:%M:%S UTC"),
            kg_index.version.as_deref().unwrap_or("N/A")
        );

        Self {
            id: uuid::Uuid::new_v4().to_string(),
            context_type: ContextType::KGIndex,
            title: format!("KG Index: {}", kg_index.name),
            summary: Some(format!(
                "Complete knowledge graph index with {} terms, {} nodes, and {} edges",
                kg_index.total_terms, kg_index.total_nodes, kg_index.total_edges
            )),
            content,
            metadata,
            created_at: chrono::Utc::now(),
            relevance_score: Some(1.0), // High relevance for complete index
        }
    }
}

/// Knowledge Graph term definition with comprehensive metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(Tsify))]
#[cfg_attr(feature = "typescript", tsify(into_wasm_abi, from_wasm_abi))]
pub struct KGTermDefinition {
    /// The primary term
    pub term: String,
    /// Normalized term value
    pub normalized_term: NormalizedTermValue,
    /// Unique identifier for the term
    pub id: u64,
    /// Definition of the term
    pub definition: Option<String>,
    /// Synonyms for the term
    pub synonyms: Vec<String>,
    /// Related terms
    pub related_terms: Vec<String>,
    /// Usage examples
    pub usage_examples: Vec<String>,
    /// URL reference if available
    pub url: Option<String>,
    /// Additional metadata
    pub metadata: AHashMap<String, String>,
    /// Relevance score for ranking
    pub relevance_score: Option<f64>,
}

/// Knowledge Graph index information
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(Tsify))]
#[cfg_attr(feature = "typescript", tsify(into_wasm_abi, from_wasm_abi))]
pub struct KGIndexInfo {
    /// Name of the knowledge graph
    pub name: String,
    /// Total number of terms in the index
    pub total_terms: usize,
    /// Number of nodes in the graph
    pub total_nodes: usize,
    /// Number of edges in the graph
    pub total_edges: usize,
    /// Last updated timestamp
    pub last_updated: chrono::DateTime<chrono::Utc>,
    /// Source of the knowledge graph
    pub source: String,
    /// Version of the knowledge graph
    pub version: Option<String>,
}

/// A single message in a conversation, including metadata for cost tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(Tsify))]
#[cfg_attr(feature = "typescript", tsify(into_wasm_abi, from_wasm_abi))]
pub struct ChatMessage {
    /// Unique identifier for this message
    pub id: MessageId,
    /// Role of the message sender
    pub role: String, // "system" | "user" | "assistant"
    /// The message content
    pub content: String,
    /// Context items associated with this message
    pub context_items: Vec<ContextItem>,
    /// Timestamp when the message was created
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Token count for this message (if available)
    pub token_count: Option<u32>,
    /// Model used to generate this message (for assistant messages)
    pub model: Option<String>,
}

impl ChatMessage {
    /// Create a new user message
    pub fn user(content: String) -> Self {
        Self {
            id: MessageId::new(),
            role: "user".to_string(),
            content,
            context_items: Vec::new(),
            created_at: chrono::Utc::now(),
            token_count: None,
            model: None,
        }
    }

    /// Create a new assistant message
    pub fn assistant(content: String, model: Option<String>) -> Self {
        Self {
            id: MessageId::new(),
            role: "assistant".to_string(),
            content,
            context_items: Vec::new(),
            created_at: chrono::Utc::now(),
            token_count: None,
            model,
        }
    }

    /// Create a new system message
    pub fn system(content: String) -> Self {
        Self {
            id: MessageId::new(),
            role: "system".to_string(),
            content,
            context_items: Vec::new(),
            created_at: chrono::Utc::now(),
            token_count: None,
            model: None,
        }
    }

    /// Add context item to this message
    pub fn add_context(&mut self, context: ContextItem) {
        self.context_items.push(context);
    }

    /// Add multiple context items to this message
    pub fn add_contexts(&mut self, contexts: Vec<ContextItem>) {
        self.context_items.extend(contexts);
    }
}

/// Health status of conversation context based on token budget utilization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(Tsify))]
#[cfg_attr(feature = "typescript", tsify(into_wasm_abi, from_wasm_abi))]
#[serde(rename_all = "snake_case")]
pub enum RotStatus {
    /// Context well within budget (< 75%)
    Fresh,
    /// Context approaching budget limit (75-90%)
    Warning,
    /// Context critically close to or over budget (> 90%)
    Critical,
}

impl std::fmt::Display for RotStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RotStatus::Fresh => write!(f, "fresh"),
            RotStatus::Warning => write!(f, "warning"),
            RotStatus::Critical => write!(f, "critical"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(Tsify))]
#[cfg_attr(feature = "typescript", tsify(into_wasm_abi, from_wasm_abi))]
pub struct Conversation {
    /// Unique identifier for this conversation
    pub id: ConversationId,
    /// Human-readable title for the conversation
    pub title: String,
    /// Messages in this conversation
    pub messages: Vec<ChatMessage>,
    /// Global context items for the entire conversation
    pub global_context: Vec<ContextItem>,
    /// Role used for this conversation
    pub role: RoleName,
    /// When this conversation was created
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// When this conversation was last updated
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// Metadata about the conversation
    pub metadata: AHashMap<String, String>,
    /// Token budget for this conversation (if set)
    #[serde(default)]
    pub token_budget: Option<usize>,
}

impl Conversation {
    /// Create a new conversation
    pub fn new(title: String, role: RoleName) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: ConversationId::new(),
            title,
            messages: Vec::new(),
            global_context: Vec::new(),
            role,
            created_at: now,
            updated_at: now,
            metadata: AHashMap::new(),
            token_budget: None,
        }
    }

    /// Set the token budget for this conversation.
    pub fn with_token_budget(mut self, budget: usize) -> Self {
        self.token_budget = Some(budget);
        self
    }

    /// Check the rot status of this conversation based on token budget.
    ///
    /// Returns `None` if no token budget is set.
    /// Uses `estimated_context_length()` as a conservative proxy for token count
    /// (byte count is always >= token count, so this errs on the side of caution).
    pub fn check_rot(&self) -> Option<RotStatus> {
        let budget = self.token_budget?;
        if budget == 0 {
            return Some(RotStatus::Critical);
        }

        let current_size = self.estimated_context_length();
        // Conservative ratio: bytes are always >= tokens, so we may flag
        // rot slightly earlier than a true tokenizer would.
        let ratio = current_size as f32 / budget as f32;

        if ratio > 0.9 {
            Some(RotStatus::Critical)
        } else if ratio > 0.75 {
            Some(RotStatus::Warning)
        } else {
            Some(RotStatus::Fresh)
        }
    }

    /// Add a message to the conversation
    pub fn add_message(&mut self, message: ChatMessage) {
        self.messages.push(message);
        self.updated_at = chrono::Utc::now();
    }

    /// Add global context to the conversation
    pub fn add_global_context(&mut self, context: ContextItem) {
        self.global_context.push(context);
        self.updated_at = chrono::Utc::now();
    }

    /// Get the total context length (approximation)
    pub fn estimated_context_length(&self) -> usize {
        let message_length: usize = self
            .messages
            .iter()
            .map(|m| {
                m.content.len()
                    + m.context_items
                        .iter()
                        .map(|c| c.content.len())
                        .sum::<usize>()
            })
            .sum();
        let global_context_length: usize =
            self.global_context.iter().map(|c| c.content.len()).sum();
        message_length + global_context_length
    }
}

/// Summary of a conversation for listing purposes
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(Tsify))]
#[cfg_attr(feature = "typescript", tsify(into_wasm_abi, from_wasm_abi))]
pub struct ConversationSummary {
    /// Unique identifier for this conversation
    pub id: ConversationId,
    /// Human-readable title for the conversation
    pub title: String,
    /// Role used for this conversation
    pub role: RoleName,
    /// Number of messages in the conversation
    pub message_count: usize,
    /// Number of context items in the conversation
    pub context_count: usize,
    /// When this conversation was created
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// When this conversation was last updated
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// Preview of the first user message (if any)
    pub preview: Option<String>,
}

// Note: Persistable implementation for Conversation will be added in the service layer
// to avoid circular dependencies

impl From<&Conversation> for ConversationSummary {
    fn from(conversation: &Conversation) -> Self {
        let context_count = conversation.global_context.len()
            + conversation
                .messages
                .iter()
                .map(|m| m.context_items.len())
                .sum::<usize>();

        let preview = conversation
            .messages
            .iter()
            .find(|m| m.role == "user")
            .map(|m| preview(&m.content, 100, "..."));

        Self {
            id: conversation.id.clone(),
            title: conversation.title.clone(),
            role: conversation.role.clone(),
            message_count: conversation.messages.len(),
            context_count,
            created_at: conversation.created_at,
            updated_at: conversation.updated_at,
            preview,
        }
    }
}

/// Context history that tracks what context has been used across conversations
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(Tsify))]
#[cfg_attr(feature = "typescript", tsify(into_wasm_abi, from_wasm_abi))]
pub struct ContextHistory {
    /// Items that have been used in conversations
    pub used_contexts: Vec<ContextHistoryEntry>,
    /// Maximum number of history entries to keep
    pub max_entries: usize,
}

impl ContextHistory {
    /// Creates an empty context history with the given capacity limit.
    pub fn new(max_entries: usize) -> Self {
        Self {
            used_contexts: Vec::new(),
            max_entries,
        }
    }

    /// Record that a context item was used
    pub fn record_usage(
        &mut self,
        context_id: &str,
        conversation_id: &ConversationId,
        usage_type: ContextUsageType,
    ) {
        let entry = ContextHistoryEntry {
            context_id: context_id.to_string(),
            conversation_id: conversation_id.clone(),
            usage_type,
            used_at: chrono::Utc::now(),
            usage_count: 1,
        };

        // Check if we already have this context for this conversation
        if let Some(existing) = self
            .used_contexts
            .iter_mut()
            .find(|e| e.context_id == context_id && e.conversation_id == *conversation_id)
        {
            existing.usage_count += 1;
            existing.used_at = chrono::Utc::now();
        } else {
            self.used_contexts.push(entry);
        }

        // Trim to max entries if needed
        if self.used_contexts.len() > self.max_entries {
            self.used_contexts.sort_by_key(|e| e.used_at);
            self.used_contexts
                .drain(0..self.used_contexts.len() - self.max_entries);
        }
    }

    /// Get frequently used contexts
    pub fn get_frequent_contexts(&self, limit: usize) -> Vec<&ContextHistoryEntry> {
        let mut entries = self.used_contexts.iter().collect::<Vec<_>>();
        entries.sort_by_key(|e| std::cmp::Reverse(e.usage_count));
        entries.into_iter().take(limit).collect()
    }
}

/// Entry in context usage history
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(Tsify))]
#[cfg_attr(feature = "typescript", tsify(into_wasm_abi, from_wasm_abi))]
pub struct ContextHistoryEntry {
    /// ID of the context item that was used
    pub context_id: String,
    /// Conversation where it was used
    pub conversation_id: ConversationId,
    /// How the context was used
    pub usage_type: ContextUsageType,
    /// When it was used
    pub used_at: chrono::DateTime<chrono::Utc>,
    /// How many times it's been used in this conversation
    pub usage_count: usize,
}

/// How a context item was used
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "typescript", derive(Tsify))]
#[cfg_attr(feature = "typescript", tsify(into_wasm_abi, from_wasm_abi))]
pub enum ContextUsageType {
    /// Added manually by user
    Manual,
    /// Added automatically by system
    Automatic,
    /// Added from search results
    SearchResult,
    /// Added from document reference
    DocumentReference,
}
