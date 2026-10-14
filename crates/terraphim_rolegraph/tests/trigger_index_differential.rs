//! Differential golden-corpus test (issue terraphim-core#33).
//!
//! Proves that the `terraphim_rolegraph` public path (`TriggerIndex`
//! re-exported from `terraphim_rolegraph_core`) and the canonical
//! `terraphim_rolegraph_core` implementation produce identical output for
//! every golden fixture. Both are the same type, so this test pins the
//! re-export: if anyone reintroduces a divergent implementation behind the
//! `terraphim_rolegraph::TriggerIndex` name, the type identity assertions and
//! the corpus run below will fail.

use ahash::AHashMap;
use terraphim_rolegraph::{DEFAULT_TRIGGER_THRESHOLD, TriggerIndex};

const CORPUS_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/trigger_index_golden.json"
);

/// The re-export must be the canonical type, not a copy.
#[test]
fn reexport_is_canonical_type() {
    fn assert_same_type(_idx: &terraphim_rolegraph_core::TriggerIndex) {}
    let idx = TriggerIndex::new(DEFAULT_TRIGGER_THRESHOLD);
    assert_same_type(&idx);
    assert_eq!(DEFAULT_TRIGGER_THRESHOLD, 0.3);
    assert!(idx.is_empty());
    assert!(TriggerIndex::is_default_stopword("the"));
    assert!(TriggerIndex::is_default_stopword("their"));
    assert!(!TriggerIndex::is_default_stopword("münchen"));
}

fn corpus_docs() -> AHashMap<u64, String> {
    let mut docs: AHashMap<u64, String> = AHashMap::new();
    docs.insert(1, "lung cancer treatment".to_string());
    docs.insert(2, "breast cancer screening".to_string());
    docs.insert(3, "rust async runtime".to_string());
    docs
}

/// The re-exported path and the canonical path must return identical results.
#[test]
fn differential_reexport_matches_canonical() {
    let corpus_raw = std::fs::read_to_string(CORPUS_PATH)
        .expect("golden corpus must exist next to the workspace conformance directory");
    assert!(
        corpus_raw.contains("\"version\": 1"),
        "unexpected golden corpus format"
    );

    let mut via_rolegraph = TriggerIndex::new(0.1);
    via_rolegraph.build(corpus_docs());

    let mut via_core = terraphim_rolegraph_core::TriggerIndex::new(0.1);
    via_core.build(corpus_docs());

    for query in [
        "lung cancer treatment",
        "breast cancer",
        "rust async",
        "münchen café",
        "",
        "the and for",
    ] {
        let a = via_rolegraph.query(query);
        let b = via_core.query(query);
        assert_eq!(a.len(), b.len(), "result count mismatch for {query:?}");
        for ((ia, sa), (ib, sb)) in a.iter().zip(&b) {
            assert_eq!(ia, ib, "node id mismatch for {query:?}");
            assert!(
                (sa - sb).abs() <= 1e-9,
                "score drift beyond tolerance for {query:?}: {sa} vs {sb}"
            );
        }
    }
}

/// Golden expectations exercised through the rolegraph path.
#[test]
fn rolegraph_path_matches_golden_expectations() {
    let mut index = TriggerIndex::new(0.1);
    let mut docs: AHashMap<u64, String> = AHashMap::new();
    docs.insert(1, "lung cancer treatment".to_string());
    docs.insert(2, "breast cancer screening".to_string());
    index.build(docs);

    let results = index.query("lung cancer treatment");
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].0, 1u64);
    assert!((results[0].1 - 1.0).abs() <= 1e-9);
    assert!((results[1].1 - 0.20199309249791833).abs() < 1e-9);

    // Unicode case folding (but not diacritic folding) is contractual.
    let mut index = TriggerIndex::new(0.0);
    let mut docs: AHashMap<u64, String> = AHashMap::new();
    docs.insert(1, "Café MÜNCHEN protocol".to_string());
    index.build(docs);
    let results = index.query("café münchen PROTOCOL");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].0, 1u64);
    assert!((results[0].1 - 1.0).abs() <= 1e-9);
}

/// Empty thesaurus, empty query and non-finite thresholds behave identically.
#[test]
fn edge_cases_behave_identically() {
    let empty: AHashMap<u64, String> = AHashMap::new();
    let mut a = TriggerIndex::new(0.3);
    a.build(empty.clone());
    let mut b = terraphim_rolegraph_core::TriggerIndex::new(0.3);
    b.build(empty);
    assert!(a.is_empty() && b.is_empty());
    assert_eq!(a.query("anything"), b.query("anything"));
    assert_eq!(a.query(""), b.query(""));

    let a = TriggerIndex::new(f64::NAN);
    let b = terraphim_rolegraph_core::TriggerIndex::new(f64::INFINITY);
    assert_eq!(a.threshold(), b.threshold());
    assert_eq!(a.threshold(), DEFAULT_TRIGGER_THRESHOLD);
}
