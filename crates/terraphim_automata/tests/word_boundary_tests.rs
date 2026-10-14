//! Regression tests for word-boundary-aware `find_matches`.
//!
//! Reproduces the defect where a legitimate two-letter thesaurus term (`ce`,
//! the Odilo KG abbreviation for "capability evaluation") matched *inside*
//! longer words, so that a query guaranteed to match nothing --
//! `concept-that-should-not-exist-71` -- still reported
//! `concepts_matched: ["ce"]` and therefore `wildcard_fallback: false`.

use terraphim_automata::{compute_concepts_matched, find_matches};
use terraphim_types::{NormalizedTerm, NormalizedTermValue, Thesaurus};

/// Build a real thesaurus containing the terms that triggered the defect.
fn thesaurus() -> Thesaurus {
    let mut t = Thesaurus::new("word-boundary-test".to_string());
    for (id, term) in [(1u64, "ce"), (2, "uv"), (3, "graph"), (4, "rust")] {
        let value = NormalizedTermValue::from(term);
        t.insert(value.clone(), NormalizedTerm::new(id, value));
    }
    t
}

fn matched_terms(text: &str) -> Vec<String> {
    let mut terms: Vec<String> = find_matches(text, &thesaurus(), false)
        .expect("find_matches must succeed")
        .into_iter()
        .map(|m| m.term)
        .collect();
    terms.sort();
    terms
}

#[test]
fn short_term_does_not_match_inside_a_longer_word() {
    // "ce" sits inside "con-ce-pt"; it must not be reported.
    assert!(matched_terms("concept-that-should-not-exist-71").is_empty());
    assert!(matched_terms("concept").is_empty());
    // "uv" inside "uvula"; "graph" inside "rolegraph"; "rust" inside "crusty".
    assert!(matched_terms("uvula").is_empty());
    assert!(matched_terms("rolegraph").is_empty());
    assert!(matched_terms("crusty").is_empty());
}

#[test]
fn short_term_still_matches_as_a_standalone_word() {
    assert_eq!(matched_terms("the ce pipeline"), vec!["ce"]);
    assert_eq!(matched_terms("ce"), vec!["ce"]);
    assert_eq!(matched_terms("run uv sync"), vec!["uv"]);
}

#[test]
fn punctuation_and_hyphens_count_as_boundaries() {
    assert_eq!(matched_terms("capability evaluation (ce)"), vec!["ce"]);
    assert_eq!(matched_terms("ce-review"), vec!["ce"]);
    assert_eq!(matched_terms("scoring,ce,done"), vec!["ce"]);
    // Underscore is a word continuation, so this must NOT match.
    assert!(matched_terms("ce_review").is_empty());
}

#[test]
fn multiple_distinct_terms_are_all_reported() {
    assert_eq!(
        matched_terms("ce and graph in rust"),
        vec!["ce", "graph", "rust"]
    );
}

#[test]
fn positions_are_still_returned_for_surviving_matches() {
    let matches = find_matches("the ce pipeline", &thesaurus(), true).expect("must succeed");
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].pos, Some((4, 6)));
}

#[test]
fn compute_concepts_matched_is_empty_for_a_nonsense_query() {
    // The exact query from the bug report.
    let concepts = compute_concepts_matched("concept-that-should-not-exist-71", &thesaurus());
    assert!(
        concepts.is_empty(),
        "nonsense query must match no concepts, got {concepts:?}"
    );
}

#[test]
fn non_ascii_scripts_still_match_without_whitespace_delimiters() {
    // CJK has no whitespace word boundaries; such terms must keep matching.
    let mut t = Thesaurus::new("cjk".to_string());
    let value = NormalizedTermValue::from("知识");
    t.insert(value.clone(), NormalizedTerm::new(9, value));

    let matches = find_matches("这是知识图谱", &t, false).expect("must succeed");
    assert_eq!(matches.len(), 1, "CJK term must match mid-string");
}
