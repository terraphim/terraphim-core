//! Tests for the allocation-free positions API (Refs #87):
//! `CompiledMatcher::find_positions`, `push_positions`, `pattern` and `term`.
//!
//! The oracle is `CompiledMatcher::find_matches(text, true)`: for every
//! option combination the positions API must report exactly the same
//! matches, in the same order (`push_positions`, and `find_positions` in
//! leftmost-longest mode) or the same set (`find_positions` in `All` mode,
//! which yields in automaton order).

use terraphim_automata::{
    CompiledMatcher, MatchOverlap, MatcherBuilder, MatcherOptions, PositionedMatch, find_matches,
};
use terraphim_types::{NormalizedTerm, NormalizedTermValue, Thesaurus};

const PATTERNS: &[&str] = &[
    "rust",
    "rust async",
    "async",
    "async runtime",
    "run",
    "time",
    "ab",
    "aba",
    "bab",
    "cat",
    "café",
    "日本",
    "日本語",
    "graph",
    "knowledge graph",
];

const BLANK_PATTERNS: &[&str] = &["  ", "\t\t"];

const TEXTS: &[&str] = &[
    "",
    "rust",
    "Rust async runtime and RUST ASYNC",
    "the async runtime runs in time; runtime",
    "ababababa bab aba",
    "concatenate the CAT, a cat and a Café or café",
    "日本語のテキスト 日本 and 日本語",
    "a knowledge graph is a graph; knowledge-graph",
    "words    with  gaps\t\tand\t\t\ttabs  ",
    "    ",
    "rust_async rust-async async_runtime",
];

fn build(options: MatcherOptions) -> CompiledMatcher {
    let allow_blank = options.allow_blank_patterns;
    let mut builder = MatcherBuilder::new(options);
    let blanks: &[&str] = if allow_blank { BLANK_PATTERNS } else { &[] };
    for (id, pattern) in PATTERNS.iter().chain(blanks).enumerate() {
        builder
            .insert(
                (*pattern).to_string(),
                NormalizedTerm::new(id as u64 + 1, NormalizedTermValue::from(*pattern)),
            )
            .unwrap();
    }
    builder.build().unwrap()
}

fn every_option_combination() -> Vec<MatcherOptions> {
    let mut all = Vec::new();
    for overlap in [MatchOverlap::LeftmostLongest, MatchOverlap::All] {
        for word_boundaries in [true, false] {
            for case_insensitive in [true, false] {
                for allow_blank_patterns in [false, true] {
                    all.push(
                        MatcherOptions::default()
                            .with_overlap(overlap)
                            .with_word_boundaries(word_boundaries)
                            .with_case_insensitive(case_insensitive)
                            .with_allow_blank_patterns(allow_blank_patterns),
                    );
                }
            }
        }
    }
    all
}

type Resolved = (String, NormalizedTerm, (usize, usize));

fn resolve(matcher: &CompiledMatcher, positions: &[PositionedMatch]) -> Vec<Resolved> {
    positions
        .iter()
        .map(|m| {
            let pattern = matcher.pattern(m.pattern_index).expect("index in range");
            let term = matcher.term(m.pattern_index).expect("index in range");
            assert_eq!(&matcher.patterns()[m.pattern_index].0, pattern);
            assert_eq!(m.range(), m.start..m.end);
            (pattern.to_string(), term.clone(), (m.start, m.end))
        })
        .collect()
}

fn oracle(matcher: &CompiledMatcher, text: &str) -> Vec<Resolved> {
    matcher
        .find_matches(text, true)
        .unwrap()
        .into_iter()
        .map(|m| {
            (
                m.term,
                m.normalized_term,
                m.pos.expect("positions requested"),
            )
        })
        .collect()
}

#[test]
fn positions_match_find_matches_for_every_option_combination() {
    let mut checked = 0usize;
    let mut non_empty = 0usize;
    for options in every_option_combination() {
        let matcher = build(options.clone());
        let mut buffer = Vec::new();
        for text in TEXTS {
            let expected = oracle(&matcher, text);
            if !expected.is_empty() {
                non_empty += 1;
            }

            buffer.clear();
            matcher.push_positions(text, &mut buffer);
            assert_eq!(
                resolve(&matcher, &buffer),
                expected,
                "push_positions, {options:?}, {text:?}"
            );

            let mut lazy: Vec<PositionedMatch> = matcher.find_positions(text).collect();
            if options.overlap == MatchOverlap::All {
                lazy.sort_unstable();
            }
            assert_eq!(
                resolve(&matcher, &lazy),
                expected,
                "find_positions, {options:?}, {text:?}"
            );
            assert_eq!(lazy, buffer);
            checked += 1;
        }
    }
    assert_eq!(checked, 16 * TEXTS.len());
    assert!(
        non_empty > 100,
        "oracle must exercise real matches: {non_empty}"
    );
}

#[test]
fn default_positions_match_the_free_find_matches() {
    let mut thesaurus = Thesaurus::new("positions".to_string());
    for (id, pattern) in PATTERNS.iter().enumerate() {
        thesaurus.insert(
            NormalizedTermValue::from(*pattern),
            NormalizedTerm::new(id as u64 + 1, NormalizedTermValue::from(*pattern)),
        );
    }
    let matcher = CompiledMatcher::from_thesaurus(&thesaurus, MatcherOptions::default()).unwrap();
    for text in TEXTS {
        let expected: Vec<Resolved> = find_matches(text, &thesaurus, true)
            .unwrap()
            .into_iter()
            .map(|m| (m.term, m.normalized_term, m.pos.unwrap()))
            .collect();
        let positions: Vec<PositionedMatch> = matcher.find_positions(text).collect();
        assert_eq!(resolve(&matcher, &positions), expected, "{text:?}");
    }
}

#[test]
fn leftmost_longest_find_positions_is_in_find_matches_order() {
    let matcher = build(MatcherOptions::default().with_word_boundaries(false));
    let text = "ababababa rust async runtime";
    let lazy: Vec<PositionedMatch> = matcher.find_positions(text).collect();
    assert!(lazy.windows(2).all(|pair| pair[0].end <= pair[1].start));
    assert_eq!(resolve(&matcher, &lazy), oracle(&matcher, text));
}

#[test]
fn all_mode_push_positions_is_sorted_and_find_positions_is_end_ordered() {
    let matcher = build(
        MatcherOptions::default()
            .with_overlap(MatchOverlap::All)
            .with_word_boundaries(false),
    );
    let text = "async runtime";
    let lazy: Vec<PositionedMatch> = matcher.find_positions(text).collect();
    assert!(lazy.windows(2).all(|pair| pair[0].end <= pair[1].end));
    let mut sorted = Vec::new();
    matcher.push_positions(text, &mut sorted);
    assert!(sorted.windows(2).all(|pair| pair[0] < pair[1]));
    // "async runtime" overlaps "async", "run" and "time": the automaton
    // reports by end offset, so its order differs from the sorted one.
    assert_ne!(lazy, sorted);
    let mut resorted = lazy.clone();
    resorted.sort_unstable();
    assert_eq!(resorted, sorted);
}

#[test]
fn push_positions_appends_and_sorts_only_the_new_tail() {
    let matcher = build(
        MatcherOptions::default()
            .with_overlap(MatchOverlap::All)
            .with_word_boundaries(false),
    );
    let mut buffer = Vec::new();
    matcher.push_positions("time async", &mut buffer);
    let first = buffer.clone();
    assert!(!first.is_empty());
    matcher.push_positions("aba", &mut buffer);
    assert_eq!(&buffer[..first.len()], &first[..]);
    let mut second = Vec::new();
    matcher.push_positions("aba", &mut second);
    assert_eq!(&buffer[first.len()..], &second[..]);
    // The concatenation is not globally sorted: offsets restart per query.
    assert!(buffer[first.len()].start < first[first.len() - 1].start);
}

#[test]
fn pattern_and_term_lookups_are_bounds_checked() {
    let matcher = build(MatcherOptions::default());
    assert_eq!(matcher.pattern(0), Some("rust"));
    assert_eq!(
        matcher.term(1).map(|t| t.value.as_str()),
        Some("rust async")
    );
    assert_eq!(matcher.pattern(PATTERNS.len()), None);
    assert!(matcher.term(usize::MAX).is_none());
}

#[test]
fn positions_on_empty_matcher_and_empty_text() {
    let empty = MatcherBuilder::new(MatcherOptions::default())
        .build()
        .unwrap();
    assert_eq!(empty.find_positions("rust").count(), 0);
    let matcher = build(MatcherOptions::default().with_overlap(MatchOverlap::All));
    let mut buffer = Vec::new();
    matcher.push_positions("", &mut buffer);
    assert!(buffer.is_empty());
}

#[test]
fn positioned_match_is_copy_and_ordered_by_start_end_index() {
    fn assert_copy<T: Copy>() {}
    assert_copy::<PositionedMatch>();
    let matcher = build(
        MatcherOptions::default()
            .with_overlap(MatchOverlap::All)
            .with_word_boundaries(false),
    );
    let mut buffer = Vec::new();
    matcher.push_positions("ababa", &mut buffer);
    let keys: Vec<(usize, usize, usize)> = buffer
        .iter()
        .map(|m| (m.start, m.end, m.pattern_index))
        .collect();
    let mut sorted = keys.clone();
    sorted.sort_unstable();
    assert_eq!(keys, sorted);
}

#[test]
fn positions_compare_against_constructed_values() {
    let matcher = build(
        MatcherOptions::default()
            .with_overlap(MatchOverlap::All)
            .with_word_boundaries(false),
    );
    let ab = PATTERNS.iter().position(|p| *p == "ab").unwrap();
    let aba = PATTERNS.iter().position(|p| *p == "aba").unwrap();
    let mut buffer = Vec::new();
    matcher.push_positions("aba", &mut buffer);
    assert_eq!(
        buffer,
        vec![
            PositionedMatch::new(0, 2, ab),
            PositionedMatch::new(0, 3, aba)
        ]
    );
}
