//! Compatibility fixtures for the domain decomposition of `terraphim_types`.
//!
//! These tests pin the historical flat public API surface: after the internal
//! decomposition into `role`, `term`, `graph`, `document`, `route`, `search`,
//! `conversation`, `routing`, `agent` and `ontology` modules, every existing
//! import path and serialised shape must keep compiling and round-tripping.

use terraphim_types::{
    AgentCommunication, AgentInfo, Concept, ContextItem, ContextType, Conversation, ConversationId,
    Document, DocumentType, Edge, IndexedDocument, KGIndexInfo, KGTermDefinition, Layer,
    LogicalOperator, MessageId, MultiAgentContext, Node, NormalizationMethod, NormalizedTerm,
    NormalizedTermValue, Priority, RelevanceFunction, RoleName, RouteDirective, RoutingDecision,
    RoutingRule, RoutingScenario, SearchQuery, Thesaurus,
};

/// Existing flat import paths must remain usable across all domains.
#[test]
fn public_paths_preserved() {
    // Roles and terms
    let role = RoleName::new("engineer");
    let value = NormalizedTermValue::from("Rust");
    let term = NormalizedTerm::with_auto_id(value.clone());
    let concept = Concept::with_stable_id(value.clone());
    assert_eq!(value.as_str(), "rust");

    // Graph
    let edge = Edge::new(1, "doc".to_string());
    let node = Node::new(2, edge);
    let mut thesaurus = Thesaurus::new("t".to_string());
    thesaurus.insert(value.clone(), term);
    assert_eq!(concept.value.as_str(), "rust");

    // Documents and routing directives
    let doc = Document {
        id: "d".to_string(),
        url: "u".to_string(),
        title: "T".to_string(),
        body: "B".to_string(),
        description: None,
        summarization: None,
        stub: None,
        tags: None,
        rank: None,
        source_haystack: None,
        doc_type: DocumentType::KgEntry,
        synonyms: None,
        route: None,
        priority: None,
        quality_score: None,
    };
    let directive = RouteDirective {
        provider: "p".to_string(),
        model: "m".to_string(),
        action: None,
        is_free: false,
    };
    assert_eq!(directive.provider, "p");

    // Search
    let query = SearchQuery::with_terms_and_operator(
        value.clone(),
        vec![NormalizedTermValue::from("programming")],
        LogicalOperator::And,
        Some(role.clone()),
    );
    assert!(query.is_multi_term_query());
    assert_eq!(query.get_all_terms().len(), 2);
    assert_eq!(Layer::from_u8(2), Some(Layer::Two));
    let _rf = RelevanceFunction::TitleScorer;

    // Conversations
    let mut conversation = Conversation::new("c".to_string(), role.clone());
    conversation.id = ConversationId::from_string("fixed".to_string());
    let _mid = MessageId::from_string("m".to_string());
    let item = ContextItem {
        id: "i".to_string(),
        context_type: ContextType::UserInput,
        title: "t".to_string(),
        summary: None,
        content: "c".to_string(),
        metadata: Default::default(),
        created_at: chrono::Utc::now(),
        relevance_score: None,
    };
    let _ = item.title.clone();
    let kg_term = KGTermDefinition {
        term: "t".to_string(),
        normalized_term: value.clone(),
        id: 1,
        definition: None,
        synonyms: vec![],
        related_terms: vec![],
        usage_examples: vec![],
        url: None,
        metadata: Default::default(),
        relevance_score: None,
    };
    let kg_index = KGIndexInfo {
        name: "kg".to_string(),
        total_terms: 0,
        total_nodes: 0,
        total_edges: 0,
        last_updated: chrono::Utc::now(),
        source: "s".to_string(),
        version: None,
    };
    let _ = ContextItem::from_kg_term_definition(&kg_term);
    let _ = ContextItem::from_kg_index(&kg_index);

    // LLM routing
    let rule = RoutingRule::new(
        "r".to_string(),
        "R".to_string(),
        "p".to_string(),
        Priority::MEDIUM,
        "prov".to_string(),
        "model".to_string(),
    );
    let decision = RoutingDecision::default("prov".to_string(), "model".to_string());
    assert_eq!(decision.scenario, RoutingScenario::Default);
    assert_eq!(rule.priority, Priority::MEDIUM);

    // Multi-agent
    let mut multi = MultiAgentContext::new();
    multi.add_agent(AgentInfo {
        id: "a".to_string(),
        name: "A".to_string(),
        role: "reviewer".to_string(),
        capabilities: vec![],
        model: None,
    });
    let _comm = AgentCommunication {
        from_agent: "a".to_string(),
        to_agent: None,
        message: "hello".to_string(),
        timestamp: chrono::Utc::now(),
    };

    // Ontology grounding
    let _method = NormalizationMethod::Exact;

    // Index and IndexedDocument remain interlinked
    let mut index = terraphim_types::Index::new();
    index.insert(doc.id.clone(), doc.clone());
    let indexed = IndexedDocument::from_document(doc);
    assert_eq!(indexed.rank, 0);
    assert_eq!(node.id, 2);
}

/// Canonical serialised shapes are unchanged by the module moves.
#[test]
fn serde_shapes_unchanged() {
    // DocumentType snake_case tagging
    assert_eq!(
        serde_json::to_string(&DocumentType::KgEntry).unwrap(),
        "\"kg_entry\""
    );

    // NormalizedTerm keeps the historic `nterm` key
    let term = NormalizedTerm::with_stable_id(NormalizedTermValue::from("machine learning"));
    let json = serde_json::to_value(&term).unwrap();
    assert!(json.get("nterm").is_some());
    assert!(json.get("value").is_none());

    // RoleName serialises as a bare string
    assert_eq!(
        serde_json::to_string(&RoleName::new("DataScientist")).unwrap(),
        "\"DataScientist\""
    );

    // LogicalOperator lowercase renames
    assert_eq!(
        serde_json::to_string(&LogicalOperator::And).unwrap(),
        "\"and\""
    );

    // Layer numeric renames
    assert_eq!(serde_json::to_string(&Layer::Three).unwrap(), "\"3\"");

    // RoutingScenario renames
    assert_eq!(
        serde_json::to_string(&RoutingScenario::LongContext).unwrap(),
        "\"long_context\""
    );

    // Old JSON without the `patterns` key still deserialises
    let old = serde_json::json!({
        "id": "r", "name": "R", "pattern": "a",
        "priority": 50, "provider": "p", "model": "m",
        "tags": [], "enabled": true,
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-01T00:00:00Z"
    });
    let rule: RoutingRule = serde_json::from_value(old).unwrap();
    assert_eq!(rule.pattern, "a");

    // Document defaults for fields added later
    let doc: Document =
        serde_json::from_str(r#"{"id":"d","url":"u","title":"T","body":"B"}"#).unwrap();
    assert_eq!(doc.doc_type, DocumentType::KgEntry);
    assert!(doc.quality_score.is_none());
}
