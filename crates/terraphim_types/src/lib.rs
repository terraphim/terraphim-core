//! Core type definitions for the Terraphim AI system.
//!
//! The implementation is decomposed into cohesive domain modules; every
//! historical public path is preserved through explicit re-exports at the
//! crate root so existing consumers continue to compile unchanged:
//!
//! - **Roles**: [`RoleName`] (see [`role`])
//! - **Knowledge Graph Types**: [`Concept`], [`Node`], [`Edge`], [`Thesaurus`] (see [`term`], [`graph`])
//! - **Document Management**: [`Document`], [`Index`], [`IndexedDocument`] (see [`document`])
//! - **Routing Directives**: [`RouteDirective`] (see [`route`])
//! - **Search Operations**: [`SearchQuery`], [`LogicalOperator`], [`RelevanceFunction`] (see [`search`])
//! - **Conversation Context**: [`Conversation`], [`ChatMessage`], [`ContextItem`] (see [`conversation`])
//! - **LLM Routing**: [`RoutingRule`], [`RoutingDecision`], [`Priority`] (see [`routing`])
//! - **Multi-Agent Coordination**: [`MultiAgentContext`], [`AgentInfo`] (see [`agent`])
//! - **Dynamic Ontology**: [`SchemaSignal`], [`ExtractedEntity`], [`CoverageSignal`], [`GroundingMetadata`] (see [`ontology`])
//! - **HGNC Gene Normalization**: `HgncGene`, `HgncNormalizer` (requires `hgnc` feature)
//!
//! # Features
//!
//! - `typescript`: Enable TypeScript type generation via tsify for WASM compatibility
//!
//! # Examples
//!
//! ## Creating a Search Query
//!
//! ```
//! use terraphim_types::{SearchQuery, NormalizedTermValue, Layer, LogicalOperator, RoleName};
//!
//! // Simple single-term query
//! let query = SearchQuery {
//!     search_term: NormalizedTermValue::from("rust"),
//!     search_terms: None,
//!     operator: None,
//!     skip: None,
//!     limit: Some(10),
//!     role: Some(RoleName::new("engineer")),
//!     layer: Layer::default(),
//!     include_pinned: false,
//!     min_quality: None,
//! };
//!
//! // Multi-term AND query
//! let multi_query = SearchQuery::with_terms_and_operator(
//!     NormalizedTermValue::from("async"),
//!     vec![NormalizedTermValue::from("programming")],
//!     LogicalOperator::And,
//!     Some(RoleName::new("engineer")),
//! );
//! ```
//!
//! ## Working with Documents
//!
//! ```
//! use terraphim_types::{Document, DocumentType};
//!
//! let document = Document {
//!     id: "doc-1".to_string(),
//!     url: "https://example.com/article".to_string(),
//!     title: "Introduction to Rust".to_string(),
//!     body: "Rust is a systems programming language...".to_string(),
//!     description: Some("A guide to Rust".to_string()),
//!     summarization: None,
//!     stub: None,
//!     tags: Some(vec!["rust".to_string(), "programming".to_string()]),
//!     rank: None,
//!     source_haystack: None,
//!     doc_type: DocumentType::KgEntry,
//!     synonyms: None,
//!     route: None,
//!     priority: None,
//!     quality_score: None,
//! };
//! ```
//!
//! ## Building a Knowledge Graph
//!
//! ```
//! use terraphim_types::{Thesaurus, NormalizedTermValue, NormalizedTerm};
//!
//! let mut thesaurus = Thesaurus::new("programming".to_string());
//! thesaurus.insert(
//!     NormalizedTermValue::from("rust"),
//!     NormalizedTerm::with_auto_id(NormalizedTermValue::from("rust programming language"))
//!         .with_url("https://rust-lang.org".to_string())
//! );
//! ```

// ---------------------------------------------------------------------------
// Domain modules
// ---------------------------------------------------------------------------

// Roles
pub mod role;
pub use role::RoleName;

// Terms and concepts (knowledge graph vocabulary)
pub mod term;
pub use term::{Concept, NormalizedTerm, NormalizedTermValue};

// Knowledge graph structure
pub mod graph;
pub use graph::{Edge, Node, Thesaurus};

// Documents and indexing
pub mod document;
pub use document::{
    Document, DocumentType, Index, IndexedDocument, QualityScore, extract_first_paragraph,
};

// Routing directives from markdown front matter
pub mod route;
pub use document::MarkdownDirectives;
pub use route::RouteDirective;

// Search operations
pub mod search;
pub use search::{KnowledgeGraphInputType, Layer, LogicalOperator, RelevanceFunction, SearchQuery};

// LLM conversations and context
pub mod conversation;
pub use conversation::{
    ChatMessage, ContextHistory, ContextHistoryEntry, ContextItem, ContextType, ContextUsageType,
    Conversation, ConversationId, ConversationSummary, KGIndexInfo, KGTermDefinition, MessageId,
    RotStatus,
};

// LLM routing rules and decisions
pub mod routing;
pub use routing::{PatternMatch, Priority, RoutingDecision, RoutingRule, RoutingScenario};

// Multi-agent coordination
pub mod agent;
pub use agent::{AgentCommunication, AgentInfo, MultiAgentContext};

// Dynamic ontology and schema-first definitions
pub mod ontology;
pub use ontology::{
    CoverageSignal, ExtractedEntity, ExtractedRelationship, GroundingMetadata, NormalizationMethod,
    OntologyAntiPattern, OntologyEntityType, OntologyRelationshipType, OntologySchema,
    SchemaSignal,
};
#[cfg(feature = "medical")]
pub use ontology::{EntityType, RelationshipType};

// Medical types module (feature-gated)
#[cfg(feature = "medical")]
pub mod medical_types;
#[cfg(feature = "medical")]
pub use medical_types::*;

// HGNC Gene Normalization module (feature-gated)
#[cfg(feature = "hgnc")]
pub mod hgnc;

// Shared learning types for knowledge graph integration
#[cfg(feature = "kg-integration")]
pub mod shared_learning;

// Core invariants: validated construction, stable IDs, UTF-8-safe previews
pub mod validation;
pub use validation::{ValidationError, preview, stable_id, truncate_utf8_safe, validate_score};

// Capability-based routing types
pub mod capability;
pub use capability::*;

/// Scoring algorithms and query types for document relevance ranking.
pub mod score;

// MCP Tool types for self-learning system
pub mod mcp_tool;
pub use mcp_tool::*;

// Procedure capture types for self-learning system
pub mod procedure;
pub use procedure::*;

// Persona definition types for agent personas
pub mod persona;
pub use persona::{CharacteristicDef, PersonaDefinition, PersonaLoadError, SfiaSkillDef};

// LLM usage tracking types for cost monitoring
pub mod llm_usage;
pub use llm_usage::{LlmResult, LlmUsage, ModelPricing};

// Review finding types for multi-agent code review
pub mod review;
pub use review::{
    FindingCategory, FindingSeverity, ReviewAgentOutput, ReviewFinding, deduplicate_findings,
};

#[cfg(test)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_query_logical_operators() {
        // Test single term query (backward compatibility)
        let single_query = SearchQuery {
            search_term: NormalizedTermValue::new("rust".to_string()),
            search_terms: None,
            operator: None,
            skip: None,
            limit: Some(10),
            role: Some(RoleName::new("test")),
            layer: Layer::default(),
            include_pinned: false,
            min_quality: None,
        };

        assert!(!single_query.is_multi_term_query());
        assert_eq!(single_query.get_all_terms().len(), 1);
        assert_eq!(single_query.get_operator(), LogicalOperator::Or); // Default

        // Test multi-term query with AND operator
        let and_query = SearchQuery::with_terms_and_operator(
            NormalizedTermValue::new("machine".to_string()),
            vec![NormalizedTermValue::new("learning".to_string())],
            LogicalOperator::And,
            Some(RoleName::new("test")),
        );

        assert!(and_query.is_multi_term_query());
        assert_eq!(and_query.get_all_terms().len(), 2);
        assert_eq!(and_query.get_operator(), LogicalOperator::And);

        // Test multi-term query with OR operator
        let or_query = SearchQuery::with_terms_and_operator(
            NormalizedTermValue::new("neural".to_string()),
            vec![NormalizedTermValue::new("networks".to_string())],
            LogicalOperator::Or,
            Some(RoleName::new("test")),
        );

        assert!(or_query.is_multi_term_query());
        assert_eq!(or_query.get_all_terms().len(), 2);
        assert_eq!(or_query.get_operator(), LogicalOperator::Or);
    }

    #[test]
    fn test_logical_operator_serialization() {
        // Test LogicalOperator serialization
        let and_op = LogicalOperator::And;
        let or_op = LogicalOperator::Or;

        let and_json = serde_json::to_string(&and_op).unwrap();
        let or_json = serde_json::to_string(&or_op).unwrap();

        assert_eq!(and_json, "\"and\"");
        assert_eq!(or_json, "\"or\"");

        // Test deserialization
        let and_deser: LogicalOperator = serde_json::from_str("\"and\"").unwrap();
        let or_deser: LogicalOperator = serde_json::from_str("\"or\"").unwrap();

        assert_eq!(and_deser, LogicalOperator::And);
        assert_eq!(or_deser, LogicalOperator::Or);
    }

    #[test]
    fn test_search_query_serialization() {
        let query = SearchQuery {
            search_term: NormalizedTermValue::new("test".to_string()),
            search_terms: Some(vec![
                NormalizedTermValue::new("additional".to_string()),
                NormalizedTermValue::new("terms".to_string()),
            ]),
            operator: Some(LogicalOperator::And),
            skip: Some(0),
            limit: Some(10),
            role: Some(RoleName::new("test_role")),
            layer: Layer::default(),
            include_pinned: false,
            min_quality: None,
        };

        let json = serde_json::to_string(&query).unwrap();
        let deserialized: SearchQuery = serde_json::from_str(&json).unwrap();

        assert_eq!(query.search_term, deserialized.search_term);
        assert_eq!(query.search_terms, deserialized.search_terms);
        assert_eq!(query.operator, deserialized.operator);
        assert_eq!(query.skip, deserialized.skip);
        assert_eq!(query.limit, deserialized.limit);
        assert_eq!(query.role, deserialized.role);
    }

    #[test]
    fn test_priority_creation_and_comparison() {
        let high = Priority::HIGH;
        let medium = Priority::MEDIUM;
        let low = Priority::LOW;
        let custom = Priority::new(75);

        assert_eq!(high.value(), 80);
        assert_eq!(medium.value(), 50);
        assert_eq!(low.value(), 20);
        assert_eq!(custom.value(), 75);

        assert!(high.is_high());
        assert!(!medium.is_high());
        assert!(medium.is_medium());
        assert!(low.is_low());

        // Test ordering
        assert!(high > medium);
        assert!(medium > low);
        assert!(custom > medium);
        assert!(custom < high);

        // Test bounds
        let max = Priority::new(150);
        assert_eq!(max.value(), 100);
        let min = Priority::new(0);
        assert_eq!(min.value(), 0);
    }

    #[test]
    fn test_routing_rule_creation() {
        let rule = RoutingRule::new(
            "test-rule".to_string(),
            "Test Rule".to_string(),
            "test.*pattern".to_string(),
            Priority::HIGH,
            "openai".to_string(),
            "gpt-4".to_string(),
        )
        .with_description("A test rule for unit testing".to_string())
        .with_tag("test".to_string())
        .with_tag("example".to_string());

        assert_eq!(rule.id, "test-rule");
        assert_eq!(rule.name, "Test Rule");
        assert_eq!(rule.pattern, "test.*pattern");
        assert_eq!(rule.priority, Priority::HIGH);
        assert_eq!(rule.provider, "openai");
        assert_eq!(rule.model, "gpt-4");
        assert_eq!(
            rule.description,
            Some("A test rule for unit testing".to_string())
        );
        assert_eq!(rule.tags, vec!["test", "example"]);
        assert!(rule.enabled);
    }

    #[test]
    fn test_routing_rule_defaults() {
        let rule = RoutingRule::with_defaults(
            "default-rule".to_string(),
            "Default Rule".to_string(),
            "default".to_string(),
            "anthropic".to_string(),
            "claude-3-sonnet".to_string(),
        );

        assert_eq!(rule.priority, Priority::MEDIUM);
        assert!(rule.enabled);
        assert!(rule.tags.is_empty());
        assert!(rule.description.is_none());
    }

    #[test]
    fn test_routing_rule_single_pattern_backcompat() {
        // `new` keeps single-pattern ergonomics; `patterns` is normalised to
        // `vec![pattern]` so the two fields never disagree (issue #22).
        let rule = RoutingRule::new(
            "r".to_string(),
            "R".to_string(),
            "think".to_string(),
            Priority::HIGH,
            "p".to_string(),
            "m".to_string(),
        );
        assert_eq!(rule.pattern, "think");
        assert_eq!(rule.patterns, vec!["think".to_string()]);
    }

    #[test]
    fn test_routing_rule_new_multi() {
        // `new_multi` groups a concept with its synonyms in one rule; the
        // first pattern is mirrored into `pattern` for back-compat.
        let rule = RoutingRule::new_multi(
            "rule-think_routing".to_string(),
            "rule-think_routing".to_string(),
            vec![
                "think_routing".to_string(),
                "think".to_string(),
                "reason".to_string(),
                "plan".to_string(),
            ],
            Priority::HIGH,
            "deepseek".to_string(),
            "deepseek-reasoner".to_string(),
        );
        assert_eq!(rule.pattern, "think_routing");
        assert_eq!(rule.patterns.len(), 4);
        assert_eq!(rule.patterns[0], "think_routing");
        assert_eq!(rule.provider, "deepseek");
        assert_eq!(rule.priority, Priority::HIGH);
    }

    #[test]
    fn test_routing_rule_with_patterns_builder() {
        let rule = RoutingRule::with_defaults(
            "r".to_string(),
            "R".to_string(),
            "initial".to_string(),
            "p".to_string(),
            "m".to_string(),
        )
        .with_patterns(vec!["concept".to_string(), "syn-a".to_string()]);

        assert_eq!(rule.pattern, "concept");
        assert_eq!(
            rule.patterns,
            vec!["concept".to_string(), "syn-a".to_string()]
        );
    }

    #[test]
    fn test_routing_rule_empty_patterns_fallback() {
        // Defensive: empty patterns leaves `pattern` empty rather than panicking.
        let rule = RoutingRule::new_multi(
            "r".to_string(),
            "R".to_string(),
            Vec::new(),
            Priority::LOW,
            "p".to_string(),
            "m".to_string(),
        );
        assert_eq!(rule.pattern, "");
        assert!(rule.patterns.is_empty());
    }

    #[test]
    fn test_routing_rule_multi_pattern_serde_roundtrip() {
        // Serde round-trip must preserve both fields; JSON written by older
        // versions (no `patterns` key) deserialises via #[serde(default)] and
        // is then normalised on read by consumers that re-derive the list.
        let rule = RoutingRule::new_multi(
            "r".to_string(),
            "R".to_string(),
            vec!["a".to_string(), "b".to_string()],
            Priority::MEDIUM,
            "p".to_string(),
            "m".to_string(),
        );
        let json = serde_json::to_string(&rule).unwrap();
        let back: RoutingRule = serde_json::from_str(&json).unwrap();
        assert_eq!(back.pattern, "a");
        assert_eq!(back.patterns, vec!["a".to_string(), "b".to_string()]);

        // Old-format JSON without `patterns` still deserialises.
        let old_json = serde_json::json!({
            "id": "r", "name": "R", "pattern": "a",
            "priority": 50, "provider": "p", "model": "m",
            "tags": [], "enabled": true,
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z"
        });
        let old_back: RoutingRule = serde_json::from_value(old_json).unwrap();
        assert_eq!(old_back.pattern, "a");
        assert!(
            old_back.patterns.is_empty(),
            "serde default yields empty list; normalisation is the consumer's call via with_patterns"
        );
    }

    #[test]
    fn test_pattern_match() {
        let pattern_match = PatternMatch::new(
            "machine-learning".to_string(),
            "openai".to_string(),
            "gpt-4".to_string(),
            0.95,
            Priority::HIGH,
            "ml-rule".to_string(),
        );

        assert_eq!(pattern_match.concept, "machine-learning");
        assert_eq!(pattern_match.provider, "openai");
        assert_eq!(pattern_match.model, "gpt-4");
        assert_eq!(pattern_match.score, 0.95);
        assert_eq!(pattern_match.priority, Priority::HIGH);
        assert_eq!(pattern_match.rule_id, "ml-rule");

        // Weighted score should be score * priority_factor
        assert_eq!(pattern_match.weighted_score, 0.95 * 0.8);
    }

    #[test]
    fn test_pattern_match_simple() {
        let simple = PatternMatch::simple(
            "test".to_string(),
            "anthropic".to_string(),
            "claude-3-haiku".to_string(),
            0.8,
        );

        assert_eq!(simple.priority, Priority::MEDIUM);
        assert_eq!(simple.rule_id, "default");
        assert_eq!(simple.weighted_score, 0.8 * 0.5);
    }

    #[test]
    fn test_routing_decision() {
        let decision = RoutingDecision::new(
            "openai".to_string(),
            "gpt-4".to_string(),
            RoutingScenario::Think,
            Priority::HIGH,
            0.9,
            "High priority thinking task".to_string(),
        );

        assert_eq!(decision.provider, "openai");
        assert_eq!(decision.model, "gpt-4");
        assert_eq!(decision.scenario, RoutingScenario::Think);
        assert_eq!(decision.priority, Priority::HIGH);
        assert_eq!(decision.confidence, 0.9);
        assert_eq!(decision.reason, "High priority thinking task");
        assert!(decision.rule_id.is_none());
    }

    #[test]
    fn test_routing_decision_with_rule() {
        let decision = RoutingDecision::with_rule(
            "anthropic".to_string(),
            "claude-3-sonnet".to_string(),
            RoutingScenario::Pattern("web-search".to_string()),
            Priority::MEDIUM,
            0.85,
            "web-rule".to_string(),
            "Web search pattern matched".to_string(),
        );

        assert_eq!(decision.rule_id, Some("web-rule".to_string()));
        assert_eq!(
            decision.scenario,
            RoutingScenario::Pattern("web-search".to_string())
        );
    }

    #[test]
    fn test_routing_decision_default() {
        let default = RoutingDecision::default("openai".to_string(), "gpt-3.5-turbo".to_string());

        assert_eq!(default.provider, "openai");
        assert_eq!(default.model, "gpt-3.5-turbo");
        assert_eq!(default.scenario, RoutingScenario::Default);
        assert_eq!(default.priority, Priority::LOW);
        assert_eq!(default.confidence, 0.5);
        assert_eq!(default.reason, "Default routing");
    }

    #[test]
    fn test_routing_scenario_serialization() {
        let scenarios = vec![
            RoutingScenario::Default,
            RoutingScenario::Background,
            RoutingScenario::Think,
            RoutingScenario::LongContext,
            RoutingScenario::WebSearch,
            RoutingScenario::Image,
            RoutingScenario::Pattern("test".to_string()),
            RoutingScenario::Priority,
            RoutingScenario::Custom("special".to_string()),
        ];

        for scenario in scenarios {
            let json = serde_json::to_string(&scenario).unwrap();
            let deserialized: RoutingScenario = serde_json::from_str(&json).unwrap();
            assert_eq!(scenario, deserialized);
        }
    }

    #[test]
    fn test_routing_scenario_display() {
        assert_eq!(format!("{}", RoutingScenario::Default), "default");
        assert_eq!(format!("{}", RoutingScenario::Think), "think");
        assert_eq!(
            format!("{}", RoutingScenario::Pattern("ml".to_string())),
            "pattern:ml"
        );
        assert_eq!(
            format!("{}", RoutingScenario::Custom("test".to_string())),
            "custom:test"
        );
    }

    #[test]
    fn test_priority_serialization() {
        let priority = Priority::new(75);
        let json = serde_json::to_string(&priority).unwrap();
        let deserialized: Priority = serde_json::from_str(&json).unwrap();
        assert_eq!(priority, deserialized);
        assert_eq!(deserialized.value(), 75);
    }

    #[test]
    fn test_routing_rule_serialization() {
        let rule = RoutingRule::new(
            "serialize-test".to_string(),
            "Serialize Test".to_string(),
            "test-pattern".to_string(),
            Priority::MEDIUM,
            "provider".to_string(),
            "model".to_string(),
        );

        let json = serde_json::to_string(&rule).unwrap();
        let deserialized: RoutingRule = serde_json::from_str(&json).unwrap();
        assert_eq!(rule.id, deserialized.id);
        assert_eq!(rule.name, deserialized.name);
        assert_eq!(rule.priority, deserialized.priority);
        assert_eq!(rule.provider, deserialized.provider);
        assert_eq!(rule.model, deserialized.model);
    }

    #[test]
    fn test_document_type_serialization() {
        let types = vec![
            DocumentType::KgEntry,
            DocumentType::Document,
            DocumentType::ConfigDocument,
        ];

        for doc_type in types {
            let json = serde_json::to_string(&doc_type).unwrap();
            let deserialized: DocumentType = serde_json::from_str(&json).unwrap();
            assert_eq!(doc_type, deserialized);
        }
    }

    #[test]
    fn test_document_defaults_for_new_fields() {
        let json = r#"{
            "id":"doc-1",
            "url":"file:///tmp/doc.md",
            "title":"Doc",
            "body":"Body"
        }"#;

        let doc: Document = serde_json::from_str(json).unwrap();
        assert_eq!(doc.doc_type, DocumentType::KgEntry);
        assert!(doc.synonyms.is_none());
        assert!(doc.route.is_none());
        assert!(doc.priority.is_none());
    }

    #[test]
    fn test_ontology_schema_deserialize() {
        let json = include_str!("../test-fixtures/sample_ontology_schema.json");
        let schema: OntologySchema = serde_json::from_str(json).unwrap();
        assert_eq!(schema.name, "Publishing Domain Model");
        assert_eq!(schema.version, "1.0.0");
        assert_eq!(schema.entity_types.len(), 3);
        assert_eq!(schema.relationship_types.len(), 1);
        assert_eq!(schema.anti_patterns.len(), 1);
    }

    #[test]
    fn test_ontology_schema_to_thesaurus_entries() {
        let json = include_str!("../test-fixtures/sample_ontology_schema.json");
        let schema: OntologySchema = serde_json::from_str(json).unwrap();
        let entries = schema.to_thesaurus_entries();
        // 3 primary labels + 2 + 2 + 3 aliases = 10 entries
        assert_eq!(entries.len(), 10);
        // Check that primary labels are present
        assert!(entries.iter().any(|(_, term, _)| term == "Chapter"));
        assert!(entries.iter().any(|(_, term, _)| term == "Concept"));
        assert!(entries.iter().any(|(_, term, _)| term == "Knowledge Graph"));
        // Check that aliases are present
        assert!(entries.iter().any(|(_, term, _)| term == "section"));
        assert!(entries.iter().any(|(_, term, _)| term == "KG"));
        // Check URIs are populated
        assert!(entries.iter().all(|(_, _, url)| url.is_some()));
    }

    #[test]
    fn test_ontology_schema_category_ids() {
        let json = include_str!("../test-fixtures/sample_ontology_schema.json");
        let schema: OntologySchema = serde_json::from_str(json).unwrap();
        let ids = schema.category_ids();
        assert_eq!(ids.len(), 3);
        assert!(ids.contains(&"chapter".to_string()));
        assert!(ids.contains(&"concept".to_string()));
        assert!(ids.contains(&"knowledge_graph".to_string()));
    }

    #[test]
    fn test_ontology_schema_uri_for() {
        let json = include_str!("../test-fixtures/sample_ontology_schema.json");
        let schema: OntologySchema = serde_json::from_str(json).unwrap();
        assert_eq!(
            schema.uri_for("chapter"),
            Some("https://schema.org/Chapter".to_string())
        );
        assert_eq!(
            schema.uri_for("concept"),
            Some("https://schema.org/DefinedTerm".to_string())
        );
        assert_eq!(schema.uri_for("nonexistent"), None);
    }

    #[test]
    fn test_ontology_schema_minimal() {
        // Minimal schema with only required fields
        let json = r#"{
            "name": "Minimal",
            "version": "0.1.0",
            "entity_types": [
                {"id": "item", "label": "Item"}
            ]
        }"#;
        let schema: OntologySchema = serde_json::from_str(json).unwrap();
        assert_eq!(schema.name, "Minimal");
        assert_eq!(schema.entity_types.len(), 1);
        assert!(schema.relationship_types.is_empty());
        assert!(schema.anti_patterns.is_empty());
        assert!(schema.entity_types[0].aliases.is_empty());
        assert!(schema.entity_types[0].uri_prefix.is_none());
    }

    #[test]
    fn test_layer_enum() {
        // Test default is Layer::One
        let default: Layer = Default::default();
        assert_eq!(default, Layer::One);

        // Test from_u8
        assert_eq!(Layer::from_u8(1), Some(Layer::One));
        assert_eq!(Layer::from_u8(2), Some(Layer::Two));
        assert_eq!(Layer::from_u8(3), Some(Layer::Three));
        assert_eq!(Layer::from_u8(0), None);
        assert_eq!(Layer::from_u8(4), None);

        // Test Display
        assert_eq!(format!("{}", Layer::One), "1");
        assert_eq!(format!("{}", Layer::Two), "2");
        assert_eq!(format!("{}", Layer::Three), "3");

        // Test includes_content
        assert!(!Layer::One.includes_content());
        assert!(Layer::Two.includes_content());
        assert!(Layer::Three.includes_content());

        // Test includes_full_content
        assert!(!Layer::One.includes_full_content());
        assert!(!Layer::Two.includes_full_content());
        assert!(Layer::Three.includes_full_content());
    }

    #[test]
    fn test_extract_first_paragraph_simple() {
        let body = "First paragraph here.\n\nSecond paragraph here.";
        assert_eq!(extract_first_paragraph(body), "First paragraph here.");
    }

    #[test]
    fn test_extract_first_paragraph_with_yaml_frontmatter() {
        let body = "---\ntitle: My Document\ntags: [rust, programming]\n---\n\nThis is the actual first paragraph.\nMore content here.";
        assert_eq!(
            extract_first_paragraph(body),
            "This is the actual first paragraph."
        );
    }

    #[test]
    fn test_extract_first_paragraph_empty_lines() {
        let body = "\n\n\nFirst paragraph after empty lines.";
        assert_eq!(
            extract_first_paragraph(body),
            "First paragraph after empty lines."
        );
    }

    #[test]
    fn test_extract_first_paragraph_single_line() {
        let body = "Just one line";
        assert_eq!(extract_first_paragraph(body), "Just one line");
    }

    #[test]
    fn test_layer_serialization() {
        // Test that Layer serializes correctly
        let query = SearchQuery {
            search_term: NormalizedTermValue::new("test".to_string()),
            search_terms: None,
            operator: None,
            skip: None,
            limit: None,
            role: None,
            layer: Layer::Two,
            include_pinned: false,
            min_quality: None,
        };

        let json = serde_json::to_string(&query).unwrap();
        assert!(json.contains("\"layer\""));

        // Deserialize and check layer is preserved
        let deserialized: SearchQuery = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.layer, Layer::Two);
    }

    #[test]
    fn test_quality_score_composite() {
        // Test with all three scores
        let full_score = QualityScore {
            knowledge: Some(0.8),
            logic: Some(0.6),
            structure: Some(0.7),
            last_evaluated: None,
        };
        assert!((full_score.composite() - 0.7).abs() < f64::EPSILON); // (0.8 + 0.6 + 0.7) / 3

        // Test with two scores
        let partial_score = QualityScore {
            knowledge: Some(0.9),
            logic: None,
            structure: Some(0.5),
            last_evaluated: None,
        };
        assert!((partial_score.composite() - 0.7).abs() < f64::EPSILON); // (0.9 + 0.5) / 2

        // Test with one score
        let single_score = QualityScore {
            knowledge: Some(0.8),
            logic: None,
            structure: None,
            last_evaluated: None,
        };
        assert!((single_score.composite() - 0.8).abs() < f64::EPSILON);

        // Test with no scores (default)
        let empty_score = QualityScore::default();
        assert_eq!(empty_score.composite(), 0.0);
    }

    #[test]
    fn test_quality_score_serialization() {
        let score = QualityScore {
            knowledge: Some(0.8),
            logic: Some(0.6),
            structure: Some(0.7),
            last_evaluated: None,
        };

        let json = serde_json::to_string(&score).unwrap();
        assert!(json.contains("0.8"));
        assert!(json.contains("0.6"));
        assert!(json.contains("0.7"));

        let deserialized: QualityScore = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.knowledge, Some(0.8));
        assert_eq!(deserialized.logic, Some(0.6));
        assert_eq!(deserialized.structure, Some(0.7));
    }

    #[test]
    fn test_quality_score_default_serialization() {
        // Test that default QualityScore serializes/deserializes correctly
        let score = QualityScore::default();
        let json = serde_json::to_string(&score).unwrap();
        let deserialized: QualityScore = serde_json::from_str(&json).unwrap();
        assert!(deserialized.knowledge.is_none());
        assert!(deserialized.logic.is_none());
        assert!(deserialized.structure.is_none());
        assert!(deserialized.last_evaluated.is_none());
    }

    #[test]
    fn test_indexed_document_with_quality_score() {
        let doc = IndexedDocument {
            id: "test-doc-1".to_string(),
            matched_edges: vec![],
            rank: 10,
            tags: vec!["rust".to_string()],
            nodes: vec![1, 2],
            quality_score: Some(QualityScore {
                knowledge: Some(0.8),
                logic: Some(0.6),
                structure: Some(0.7),
                last_evaluated: None,
            }),
        };

        assert_eq!(doc.id, "test-doc-1");
        assert!((doc.quality_score.as_ref().unwrap().composite() - 0.7).abs() < f64::EPSILON);
    }

    #[test]
    fn test_indexed_document_from_document_quality_score_none() {
        let doc = Document {
            id: "doc-1".to_string(),
            url: "https://example.com".to_string(),
            title: "Test".to_string(),
            body: "Body".to_string(),
            description: None,
            summarization: None,
            stub: None,
            tags: None,
            rank: None,
            source_haystack: None,
            doc_type: DocumentType::Document,
            synonyms: None,
            route: None,
            priority: None,
            quality_score: None,
        };

        let indexed = IndexedDocument::from_document(doc);
        assert!(indexed.quality_score.is_none());
    }

    #[test]
    fn test_indexed_document_serialization_backward_compat() {
        // Test that IndexedDocument without quality_score deserializes correctly
        // This simulates old data that doesn't have the quality_score field
        // NOTE: node IDs are u64 integers
        let json = r#"{
            "id": "doc-1",
            "matched_edges": [],
            "rank": 5,
            "tags": ["test"],
            "nodes": [1]
        }"#;

        let doc: IndexedDocument = serde_json::from_str(json).unwrap();
        assert_eq!(doc.id, "doc-1");
        assert!(doc.quality_score.is_none());
    }

    #[test]
    fn test_conversation_summary_preview_is_utf8_safe() {
        // Regression: byte-index slicing at 100 panicked on multibyte input.
        let content = "é".repeat(150); // 300 bytes, 150 chars
        let mut conv = Conversation::new("t".to_string(), RoleName::new("engineer"));
        conv.add_message(ChatMessage::user(content.clone()));
        let summary = ConversationSummary::from(&conv);
        let prev = summary.preview.unwrap();
        assert_eq!(prev.chars().count(), 103); // 100 chars + "..."
        assert!(prev.ends_with("..."));

        // Short content is returned unchanged.
        let mut conv2 = Conversation::new("t".to_string(), RoleName::new("engineer"));
        conv2.add_message(ChatMessage::user("héllo".to_string()));
        let summary2 = ConversationSummary::from(&conv2);
        assert_eq!(summary2.preview.as_deref(), Some("héllo"));
    }

    #[test]
    fn test_stable_ids_are_deterministic_and_persisted() {
        let term_a = NormalizedTerm::with_stable_id(NormalizedTermValue::from("machine learning"));
        let term_b = NormalizedTerm::with_stable_id(NormalizedTermValue::from("machine learning"));
        assert_eq!(term_a.id, term_b.id);

        // Survives a serialisation round trip unchanged.
        let json = serde_json::to_string(&term_a).unwrap();
        let back: NormalizedTerm = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, term_a.id);

        let concept_a = Concept::with_stable_id(NormalizedTermValue::from("rust"));
        let concept_b = Concept::with_stable_id(NormalizedTermValue::from("rust"));
        assert_eq!(concept_a.id, concept_b.id);
        assert_ne!(concept_a.id, 0);
    }

    #[test]
    fn test_quality_score_validated_constructor() {
        let ok = QualityScore::try_new(Some(0.5), Some(0.25), None, None).unwrap();
        assert_eq!(ok.composite(), 0.375);
        assert!(QualityScore::try_new(Some(1.5), None, None, None).is_err());
        assert!(QualityScore::try_new(None, Some(-0.5), None, None).is_err());
        assert!(QualityScore::try_new(None, None, Some(f64::NAN), None).is_err());
    }

    #[test]
    fn test_thesaurus_source_hash_roundtrip() {
        let mut thesaurus = Thesaurus::new("test".to_string());
        thesaurus.source_hash = Some("abc123".to_string());

        let json = serde_json::to_string(&thesaurus).unwrap();
        let deserialized: Thesaurus = serde_json::from_str(&json).unwrap();

        assert_eq!(thesaurus.source_hash, deserialized.source_hash);
    }

    #[test]
    fn test_thesaurus_source_hash_backward_compat() {
        // Old thesaurus JSON without source_hash should deserialize with None
        let json = r#"{"name":"test","data":{}}"#;
        let thesaurus: Thesaurus = serde_json::from_str(json).unwrap();
        assert!(thesaurus.source_hash.is_none());
    }

    #[test]
    fn test_thesaurus_with_source_hash() {
        let thesaurus = Thesaurus::new("test".to_string()).with_source_hash("hash123".to_string());
        assert_eq!(thesaurus.source_hash, Some("hash123".to_string()));
    }

    #[test]
    fn test_context_rot_no_budget_returns_none() {
        let conv = Conversation::new("test".to_string(), RoleName::new("engineer"));
        assert!(conv.check_rot().is_none());
    }

    #[test]
    fn test_context_rot_fresh() {
        let conv = Conversation::new("test".to_string(), RoleName::new("engineer"))
            .with_token_budget(1000);
        // Empty conversation should be fresh
        assert_eq!(conv.check_rot(), Some(RotStatus::Fresh));
    }

    #[test]
    fn test_context_rot_warning() {
        let mut conv = Conversation::new("test".to_string(), RoleName::new("engineer"))
            .with_token_budget(1000);
        // Add enough content to hit 75-90% ratio
        // Need ~750-900 bytes to trigger warning
        let content = "x".repeat(800);
        conv.add_message(ChatMessage::user(content));
        assert_eq!(conv.check_rot(), Some(RotStatus::Warning));
    }

    #[test]
    fn test_context_rot_critical() {
        let mut conv = Conversation::new("test".to_string(), RoleName::new("engineer"))
            .with_token_budget(1000);
        // Add enough content to exceed 90% ratio
        // Need >900 bytes to trigger critical
        let content = "x".repeat(950);
        conv.add_message(ChatMessage::user(content));
        assert_eq!(conv.check_rot(), Some(RotStatus::Critical));
    }

    #[test]
    fn test_context_rot_zero_budget_is_critical() {
        let conv =
            Conversation::new("test".to_string(), RoleName::new("engineer")).with_token_budget(0);
        assert_eq!(conv.check_rot(), Some(RotStatus::Critical));
    }

    #[test]
    fn test_context_rot_display() {
        assert_eq!(format!("{}", RotStatus::Fresh), "fresh");
        assert_eq!(format!("{}", RotStatus::Warning), "warning");
        assert_eq!(format!("{}", RotStatus::Critical), "critical");
    }

    #[test]
    fn test_context_rot_serde_roundtrip() {
        let status = RotStatus::Warning;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "\"warning\"");
        let deserialized: RotStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(status, deserialized);
    }
}
