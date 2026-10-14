//! Shared golden-corpus suite for `TriggerIndex`.
//!
//! The corpus at `conformance/trigger_index_golden.json` is the behavioural
//! contract for `TriggerIndex` (issue terraphim-core#33). Every public path —
//! the canonical `terraphim_rolegraph_core::TriggerIndex` and the re-exported
//! `terraphim_rolegraph::TriggerIndex` — must produce identical output for
//! every accepted fixture.
//!
//! Regeneration of captured scores is a deliberate, reviewed act: set
//! `REGOLDEN=1` to rewrite the `expect` values from the canonical
//! implementation, inspect the diff, and record the rationale in the PR.

use ahash::AHashMap;
use serde::Deserialize;
use terraphim_rolegraph_core::TriggerIndex;

#[derive(Deserialize)]
struct Corpus {
    #[allow(dead_code)]
    version: u32,
    #[allow(dead_code)]
    description: String,
    tolerance: f64,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    name: String,
    threshold: f64,
    #[serde(default)]
    stopwords: Option<Vec<String>>,
    docs: AHashMap<u64, String>,
    queries: Vec<Query>,
}

#[derive(Deserialize)]
struct Query {
    text: String,
    /// Expected `(node_id, score)` pairs. When absent, only `expect_ids`
    /// ordering is checked.
    #[serde(default)]
    expect: Option<Vec<(u64, f64)>>,
    /// Expected node ids in deterministic order (used where exact scores are
    /// implementation-internal but ordering is contractual).
    #[serde(default)]
    expect_ids: Option<Vec<u64>>,
}

fn corpus_path() -> String {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let default = format!("{manifest}/tests/fixtures/trigger_index_golden.json");
    std::env::var("TRIGGER_GOLDEN_CORPUS").unwrap_or(default)
}

fn build_index(case: &Case) -> TriggerIndex {
    let stopwords = case
        .stopwords
        .as_ref()
        .map(|words| words.iter().cloned().collect::<ahash::AHashSet<String>>());
    match stopwords {
        Some(sw) => TriggerIndex::with_stopwords(case.threshold, sw),
        None => TriggerIndex::new(case.threshold),
    }
}

fn run_case(case: &Case, tolerance: f64) -> Result<(), String> {
    let mut index = build_index(case);
    index.build(case.docs.clone());
    for query in &case.queries {
        let results = index.query(&query.text);
        let ids: Vec<u64> = results.iter().map(|(id, _)| *id).collect();
        if let Some(expected_ids) = &query.expect_ids {
            if &ids != expected_ids {
                return Err(format!(
                    "case `{}` query `{:?}`: expected ids {expected_ids:?}, got {ids:?}",
                    case.name, query.text
                ));
            }
        }
        if let Some(expected) = &query.expect {
            if results.len() != expected.len() {
                return Err(format!(
                    "case `{}` query `{:?}`: expected {} results, got {}: {results:?}",
                    case.name,
                    query.text,
                    expected.len(),
                    results.len()
                ));
            }
            for ((got_id, got_score), (want_id, want_score)) in results.iter().zip(expected) {
                if got_id != want_id || (got_score - want_score).abs() > tolerance {
                    return Err(format!(
                        "case `{}` query `{:?}`: expected ({want_id}, {want_score}), got ({got_id}, {got_score})",
                        case.name, query.text
                    ));
                }
            }
        }
        // Scores must always be finite and non-negative.
        for (id, score) in &results {
            if !score.is_finite() || *score < 0.0 {
                return Err(format!(
                    "case `{}` query `{:?}`: invalid score {score} for node {id}",
                    case.name, query.text
                ));
            }
        }
    }
    Ok(())
}

#[test]
fn canonical_implementation_matches_golden_corpus() {
    let path = corpus_path();
    let raw =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read golden corpus {path}: {e}"));
    let corpus: Corpus = serde_json::from_str(&raw).expect("parse golden corpus");
    for case in &corpus.cases {
        if let Err(msg) = run_case(case, corpus.tolerance) {
            panic!("{msg}");
        }
    }
}
