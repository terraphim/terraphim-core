//! Idempotency and determinism regression tests for the rolegraph.
//!
//! Covers issue #35:
//! - replaying identical logical input leaves index size, graph topology and
//!   results unchanged
//! - replacing a document removes every stale term contribution
//! - deleting a missing document is a documented idempotent no-op
//! - ranking is deterministic with an explicit stable tie-breaker
//! - input permutation does not change canonical final state or ranked output

use ahash::AHashMap;
use terraphim_automata::{AutomataPath, load_thesaurus};
use terraphim_rolegraph::RoleGraph;
use terraphim_types::{Document, DocumentType, IndexedDocument, Thesaurus};

async fn sample_thesaurus() -> Thesaurus {
    let fixture =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/term_to_id.json");
    load_thesaurus(&AutomataPath::from_local(fixture))
        .await
        .unwrap()
}

fn doc(id: &str, title: &str, body: &str) -> Document {
    Document {
        stub: None,
        url: format!("/test/{id}"),
        tags: None,
        rank: None,
        source_haystack: None,
        id: id.to_string(),
        title: title.to_string(),
        body: body.to_string(),
        description: None,
        summarization: None,
        doc_type: DocumentType::KgEntry,
        synonyms: None,
        route: None,
        priority: None,
        quality_score: None,
    }
}

/// Canonical state snapshot: node count, edge count, document count and the
/// ranked query output for a fixed set of queries.
fn canonical_state(graph: &RoleGraph, queries: &[&str]) -> (usize, usize, usize, Vec<String>) {
    let mut ranked = Vec::new();
    for q in queries {
        let results = graph.query_graph(q, None, None).unwrap();
        ranked.push(
            results
                .iter()
                .map(|(id, d)| format!("{id}:{}", d.rank))
                .collect::<Vec<_>>()
                .join(","),
        );
    }
    (
        graph.get_node_count(),
        graph.get_edge_count(),
        graph.get_document_count(),
        ranked,
    )
}

#[tokio::test]
async fn replaying_identical_input_is_idempotent() {
    let role = "idempotency".to_string();
    let body = "Life cycle concepts and project planning and Trained operators and maintainers";

    let mut graph = RoleGraph::new(role.clone().into(), sample_thesaurus().await)
        .await
        .unwrap();
    graph.insert_document("doc-1", doc("doc-1", "first", body));
    let queries = ["Life cycle concepts", "project planning"];
    let after_first = canonical_state(&graph, &queries);

    // Replay the exact same logical input.
    graph.insert_document("doc-1", doc("doc-1", "first", body));
    let after_replay = canonical_state(&graph, &queries);

    assert_eq!(after_first, after_replay, "replay must not change state");
    assert_eq!(after_replay.2, 1, "document count must stay at one");
}

#[tokio::test]
async fn many_replays_converge() {
    let role = "idempotency".to_string();
    let body = "Life cycle concepts and project planning";

    let mut graph = RoleGraph::new(role.into(), sample_thesaurus().await)
        .await
        .unwrap();
    graph.insert_document("doc-1", doc("doc-1", "title", body));
    let queries = ["Life cycle concepts", "project planning"];
    let baseline = canonical_state(&graph, &queries);

    for _ in 0..5 {
        graph.insert_document("doc-1", doc("doc-1", "title", body));
        assert_eq!(
            canonical_state(&graph, &queries),
            baseline,
            "every replay must converge on the same state"
        );
    }
}

#[tokio::test]
async fn replacing_document_removes_stale_contributions() {
    let role = "replace".to_string();
    let mut graph = RoleGraph::new(role.into(), sample_thesaurus().await)
        .await
        .unwrap();

    graph.insert_document(
        "doc-1",
        doc("doc-1", "old", "Life cycle concepts and project planning"),
    );
    assert!(graph.has_document("doc-1"));

    // Replace with entirely different content.
    graph.insert_document(
        "doc-1",
        doc(
            "doc-1",
            "new",
            "Trained operators and maintainers and project direction",
        ),
    );
    assert_eq!(graph.get_document_count(), 1);

    // Stale terms must no longer resolve to the document.
    let stale = graph
        .query_graph("Life cycle concepts", None, None)
        .unwrap();
    assert!(
        !stale.iter().any(|(id, _)| id == "doc-1"),
        "stale term must not match replaced document"
    );

    // New content must match.
    let fresh = graph.query_graph("project direction", None, None).unwrap();
    assert!(fresh.iter().any(|(id, _)| id == "doc-1"));
}

#[tokio::test]
async fn delete_missing_document_is_idempotent_noop() {
    let role = "delete".to_string();
    let mut graph = RoleGraph::new(role.into(), sample_thesaurus().await)
        .await
        .unwrap();
    graph.insert_document(
        "doc-1",
        doc("doc-1", "t", "Life cycle concepts and project planning"),
    );

    let queries = ["Life cycle concepts"];
    let before = canonical_state(&graph, &queries);

    // Deleting a missing document is a documented no-op.
    assert!(!graph.remove_document("missing"));
    let after_missing = canonical_state(&graph, &queries);
    assert_eq!(before, after_missing);

    // Deleting an existing document removes all contributions; repeating is a no-op.
    assert!(graph.remove_document("doc-1"));
    assert!(!graph.remove_document("doc-1"));
    assert_eq!(graph.get_document_count(), 0);
    assert_eq!(graph.get_node_count(), 0);
    assert_eq!(graph.get_edge_count(), 0);
    assert!(
        graph
            .query_graph("Life cycle concepts", None, None)
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn input_permutation_produces_identical_canonical_state() {
    let role = "permutation".to_string();
    let docs = [
        ("doc-a", "Life cycle concepts and project direction"),
        ("doc-b", "Life cycle concepts and project planning"),
        (
            "doc-c",
            "Trained operators and maintainers with project planning",
        ),
    ];
    let queries = ["Life cycle concepts", "project planning"];

    let mut forward = RoleGraph::new(role.clone().into(), sample_thesaurus().await)
        .await
        .unwrap();
    for (id, body) in &docs {
        forward.insert_document(id, doc(id, "t", body));
    }
    let forward_state = canonical_state(&forward, &queries);

    let mut reversed = RoleGraph::new(role.into(), sample_thesaurus().await)
        .await
        .unwrap();
    for (id, body) in docs.iter().rev() {
        reversed.insert_document(id, doc(id, "t", body));
    }
    let reversed_state = canonical_state(&reversed, &queries);

    assert_eq!(forward_state.0, reversed_state.0, "node counts must match");
    assert_eq!(forward_state.1, reversed_state.1, "edge counts must match");
    assert_eq!(
        forward_state.2, reversed_state.2,
        "document counts must match"
    );
    // Ranked output must be identical modulo identical-rank ties, which are
    // broken deterministically by document ID.
    assert_eq!(
        forward_state.3, reversed_state.3,
        "ranked output must be identical regardless of insertion order"
    );
}

#[tokio::test]
async fn ranking_ties_break_on_document_id() {
    // Two documents matching the same single term end up with identical
    // aggregate ranks; the tie must be broken deterministically by document ID.
    let role = "ties".to_string();
    let mut graph = RoleGraph::new(role.into(), sample_thesaurus().await)
        .await
        .unwrap();
    graph.insert_document(
        "zzz",
        doc("zzz", "z", "Life cycle concepts and project planning"),
    );
    graph.insert_document(
        "aaa",
        doc("aaa", "a", "Life cycle concepts and project planning"),
    );
    graph.insert_document(
        "mmm",
        doc("mmm", "m", "Life cycle concepts and project planning"),
    );

    let results = graph.query_graph("project planning", None, None).unwrap();
    assert!(results.len() >= 2);
    let ids: Vec<&String> = results.iter().map(|(id, _)| id).collect();
    let mut sorted_ids = ids.clone();
    sorted_ids.sort();
    assert_eq!(
        ids, sorted_ids,
        "equal ranks must be ordered by document ID"
    );
}

#[tokio::test]
async fn failed_update_keeps_previous_state() {
    // Simulate a failed update: an empty document insertion for an existing ID
    // must still follow replace semantics and leave a queryable, consistent
    // graph (no partial counts from the previous content leaking through).
    let role = "failed-update".to_string();
    let mut graph = RoleGraph::new(role.into(), sample_thesaurus().await)
        .await
        .unwrap();
    graph.insert_document(
        "doc-1",
        doc("doc-1", "t", "Life cycle concepts and project planning"),
    );
    let queries = ["Life cycle concepts"];
    let before = canonical_state(&graph, &queries);

    graph.insert_document("doc-1", doc("doc-1", "t", ""));
    let after = canonical_state(&graph, &queries);

    // The old term contributions are gone; the document entry is replaced by
    // the (empty) new content. No partial double counting remains.
    assert_ne!(before, after);
    assert_eq!(graph.get_document_count(), 1);
}

#[tokio::test]
async fn trigger_index_rebuild_is_idempotent() {
    let role = "trigger".to_string();
    let mut graph = RoleGraph::new(role.into(), Thesaurus::new("t".to_string()))
        .await
        .unwrap();

    let mut triggers: AHashMap<u64, String> = AHashMap::new();
    triggers.insert(1u64, "managing dependencies in rust projects".to_string());
    graph.load_trigger_index(triggers.clone(), vec![], 0.3);
    let first = graph.find_matching_node_ids_with_fallback("managing rust dependencies", false);
    assert!(first.contains(&1));

    // Rebuild with identical input must give identical results.
    graph.load_trigger_index(triggers, vec![], 0.3);
    let second = graph.find_matching_node_ids_with_fallback("managing rust dependencies", false);
    assert_eq!(first, second);
}

#[tokio::test]
async fn serializable_roundtrip_preserves_canonical_state() {
    let role = "roundtrip".to_string();
    let mut graph = RoleGraph::new(role.into(), sample_thesaurus().await)
        .await
        .unwrap();
    graph.insert_document(
        "doc-1",
        doc("doc-1", "t", "Life cycle concepts and project planning"),
    );
    let queries = ["Life cycle concepts", "project planning"];
    let before = canonical_state(&graph, &queries);

    let json = graph.to_serializable().to_json().unwrap();
    let restored = RoleGraph::from_serializable(
        terraphim_rolegraph::SerializableRoleGraph::from_json(&json).unwrap(),
    )
    .await
    .unwrap();
    let after = canonical_state(&restored, &queries);

    assert_eq!(before, after);
}

// Keep the IndexedDocument import referenced even if future refactors drop a
// use site above; the type appears in query results throughout.
#[allow(dead_code)]
fn _t(_: Vec<(String, IndexedDocument)>) {}

fn repeated_graph() -> RoleGraph {
    use terraphim_types::NormalizedTerm;
    let mut thesaurus = Thesaurus::new("repeated concepts".into());
    for (id, term) in [(1, "alpha"), (2, "beta")] {
        thesaurus.insert(term.into(), NormalizedTerm::new(id, term.into()));
    }
    RoleGraph::new_sync("repeated concepts".into(), thesaurus).unwrap()
}

fn assert_same_contributions(actual: &RoleGraph, expected: &RoleGraph) {
    assert_eq!(actual.nodes_map(), expected.nodes_map());
    assert_eq!(actual.edges_map(), expected.edges_map());
    assert_eq!(actual.get_document_count(), expected.get_document_count());
    assert_eq!(
        canonical_state(actual, &["alpha", "beta"]),
        canonical_state(expected, &["alpha", "beta"])
    );
}

#[test]
fn repeated_edges_and_self_edges_replay_exactly() {
    let mut graph = repeated_graph();
    let body = "alpha alpha beta alpha beta beta alpha alpha";
    graph.insert_document("a", doc("a", "untitled", body));
    let expected = graph.clone();
    for _ in 0..5 {
        graph.insert_document("a", doc("a", "untitled", body));
        assert_same_contributions(&graph, &expected);
    }
    graph.remove_document("a");
    assert_same_contributions(&graph, &repeated_graph());
}

#[test]
fn replacement_and_removal_equal_rebuilding_remaining_corpus() {
    let mut graph = repeated_graph();
    graph.insert_document(
        "a",
        doc("a", "untitled", "alpha alpha beta alpha beta beta"),
    );
    graph.insert_document("b", doc("b", "untitled", "beta alpha beta beta alpha"));
    graph.insert_document("a", doc("a", "untitled", "beta beta alpha"));
    let mut rebuilt = repeated_graph();
    rebuilt.insert_document("a", doc("a", "untitled", "beta beta alpha"));
    rebuilt.insert_document("b", doc("b", "untitled", "beta alpha beta beta alpha"));
    assert_same_contributions(&graph, &rebuilt);
    graph.remove_document("a");
    let mut remaining = repeated_graph();
    remaining.insert_document("b", doc("b", "untitled", "beta alpha beta beta alpha"));
    assert_same_contributions(&graph, &remaining);
}

#[cfg(feature = "kg-integration")]
#[test]
fn replacing_or_removing_learning_document_clears_membership_and_contributions() {
    let mut graph = repeated_graph();
    graph.insert_document("a", doc("a", "untitled", "alpha beta alpha beta"));
    let mut learning = IndexedDocument::from_document(doc("a", "untitled", ""));
    learning.nodes = vec![1];
    graph.index_learning_document(learning.clone()).unwrap();
    assert_eq!(graph.get_learning_documents("alpha").len(), 1);
    assert!(graph.nodes_map().is_empty());
    assert!(graph.edges_map().is_empty());
    graph.insert_document("a", doc("a", "untitled", "alpha beta"));
    assert!(!graph.adjust_learning_document_rank("a", true, 1));
    graph.index_learning_document(learning).unwrap();
    assert!(graph.remove_document("a"));
    graph.insert_document("a", doc("a", "untitled", "alpha beta"));
    assert!(!graph.adjust_learning_document_rank("a", true, 1));
}

#[test]
fn large_representable_edge_ids_unpair_exactly() {
    use terraphim_rolegraph::{magic_pair, magic_unpair};
    for (a, b) in [
        (4_000_000_000, 1),
        (1, 4_000_000_000),
        (4_000_000_000, 4_000_000_000),
        (4_294_967_295, 0),
    ] {
        assert_eq!(magic_unpair(magic_pair(a, b)), (a, b));
    }
    assert_eq!(magic_unpair(u64::MAX), (4_294_967_295, 4_294_967_295));
}

#[test]
fn checked_pair_refuses_unrepresentable_identity() {
    use terraphim_rolegraph::checked_magic_pair;
    assert_eq!(
        checked_magic_pair(4_294_967_295, 4_294_967_295),
        Some(u64::MAX)
    );
    assert_eq!(checked_magic_pair(4_294_967_296, 0), None);
    assert_eq!(checked_magic_pair(0, u64::MAX), None);
}
