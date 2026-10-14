//! Tests for the compiled matcher API (`MatcherBuilder` / `CompiledMatcher`).
//!
//! Contract under test:
//! - compile validated patterns once, reuse across many queries;
//! - results are identical to the existing `find_matches` semantics;
//! - concurrent read-only use is safe;
//! - construction configuration is explicit and serialisable (no serialising
//!   of compiled implementation internals).

use std::thread;

use terraphim_automata::compiled::DEFAULT_MIN_PATTERN_LENGTH;
use terraphim_automata::{
    CompiledMatcher, MatchOverlap, Matched, MatcherBuilder, MatcherOptions, find_matches,
    thesaurus_from_terms,
};
use terraphim_types::{NormalizedTerm, NormalizedTermValue, RoleName, Thesaurus};

fn thesaurus_sample() -> Thesaurus {
    let role = RoleName::new("Engineer");
    thesaurus_from_terms(
        &role,
        [
            "rust",
            "rust async",
            "async runtime",
            "knowledge graph",
            "machine learning",
        ],
    )
}

#[test]
fn compiled_matches_equal_find_matches() {
    let thesaurus = thesaurus_sample();
    let compiled = CompiledMatcher::from_thesaurus(&thesaurus, MatcherOptions::default()).unwrap();

    let text = "Rust and RUST ASYNC power the knowledge graph with an async runtime.";
    let expected = find_matches(text, &thesaurus, true).unwrap();
    let got = compiled.find_matches(text, true).unwrap();
    assert_eq!(got, expected);
}

#[test]
fn repeated_reuse_is_identical() {
    let thesaurus = thesaurus_sample();
    let compiled = CompiledMatcher::from_thesaurus(&thesaurus, MatcherOptions::default()).unwrap();

    let first = compiled
        .find_matches("rust knowledge graph machine learning", false)
        .unwrap()
        .len();
    assert_eq!(first, 3);
    for _ in 0..50 {
        let matches = compiled
            .find_matches("rust knowledge graph machine learning", false)
            .unwrap();
        assert_eq!(matches.len(), first);
    }
}

#[test]
fn builder_skips_invalid_patterns() {
    let mut builder = MatcherBuilder::new(MatcherOptions::default());
    let _ = builder.insert("rust".to_string(), sample_term("rust"));
    let short = builder.insert("a".to_string(), sample_term("a")).is_err();
    let empty = builder.insert("  ".to_string(), sample_term("  ")).is_err();
    assert!(short, "too-short pattern must be rejected");
    assert!(empty, "blank pattern must be rejected");

    let compiled = builder.build().unwrap();
    let matches = compiled.find_matches("rust", true).unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].term, "rust");
}

#[test]
fn builder_rejects_duplicate_patterns() {
    let mut builder = MatcherBuilder::new(MatcherOptions::default());
    builder
        .insert("rust".to_string(), sample_term("rust"))
        .unwrap();
    let dup = builder.insert("rust".to_string(), sample_term("rust"));
    assert!(dup.is_err(), "duplicate patterns must be rejected");
}

#[test]
fn empty_pattern_set_builds_and_matches_nothing() {
    let builder = MatcherBuilder::new(MatcherOptions::default());
    let compiled = builder.build().unwrap();
    assert!(compiled.is_empty());
    assert!(compiled.find_matches("rust", true).unwrap().is_empty());
}

#[test]
fn empty_query_returns_no_matches() {
    let compiled =
        CompiledMatcher::from_thesaurus(&thesaurus_sample(), MatcherOptions::default()).unwrap();
    assert!(compiled.find_matches("", true).unwrap().is_empty());
}

#[test]
fn leftmost_longest_semantics_preserved() {
    // "rust async" must win over the shorter "rust" prefix.
    let compiled =
        CompiledMatcher::from_thesaurus(&thesaurus_sample(), MatcherOptions::default()).unwrap();
    let matches = compiled.find_matches("rust async", true).unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].term, "rust async");
}

#[test]
fn multibyte_unicode_positions_are_byte_accurate() {
    let mut builder = MatcherBuilder::new(MatcherOptions::default());
    builder
        .insert("héllo".to_string(), sample_term("héllo"))
        .unwrap();
    let compiled = builder.build().unwrap();

    let text = "say héllo wörld";
    let expected = {
        let role = RoleName::new("T");
        let t = thesaurus_from_terms(&role, ["héllo"]);
        find_matches(text, &t, true).unwrap()
    };
    let got = compiled.find_matches(text, true).unwrap();
    assert_eq!(got.len(), expected.len());
    assert_eq!(got[0].term, expected[0].term);
    assert_eq!(got[0].pos, expected[0].pos);
    let (start, end) = got[0].pos.unwrap();
    assert_eq!(&text[start..end], "héllo");
}

#[test]
fn overlapping_matches_reported_like_reference() {
    let role = RoleName::new("T");
    let thesaurus = thesaurus_from_terms(&role, ["abc", "bcd"]);
    let compiled = CompiledMatcher::from_thesaurus(&thesaurus, MatcherOptions::default()).unwrap();

    // Neither `abc` nor `bcd` is a whole word inside `abcd`, so both are
    // rejected by the word-boundary rule. Previously `abc` was reported here,
    // which is the same class of defect as `ce` matching inside `concept`.
    let got = compiled.find_matches("abcd", true).unwrap();
    let expected = find_matches("abcd", &thesaurus, true).unwrap();
    assert_eq!(got, expected);
    assert!(got.is_empty(), "a term inside a longer word is not a match");

    // With real boundaries the overlap rule still applies: leftmost-longest
    // picks one match per position, and the compiled matcher agrees with the
    // reference implementation.
    let got = compiled.find_matches("abc bcd", true).unwrap();
    let expected = find_matches("abc bcd", &thesaurus, true).unwrap();
    assert_eq!(got, expected);
    assert_eq!(got.len(), 2, "both terms match as standalone words");
}

#[test]
fn concurrent_read_only_use_is_safe() {
    let compiled =
        CompiledMatcher::from_thesaurus(&thesaurus_sample(), MatcherOptions::default()).unwrap();

    let handles: Vec<_> = (0..8)
        .map(|i| {
            let compiled = compiled.clone();
            thread::spawn(move || {
                let text = format!("rust async knowledge graph number {i}");
                let matches = compiled.find_matches(&text, true).unwrap();
                assert_eq!(matches.len(), 2);
                matches
            })
        })
        .collect();
    for h in handles {
        let matches = h.join().unwrap();
        assert_eq!(matches[0].term, "rust async");
        assert_eq!(matches[1].term, "knowledge graph");
    }
}

#[test]
fn caller_buffer_path_avoids_reallocation() {
    let compiled =
        CompiledMatcher::from_thesaurus(&thesaurus_sample(), MatcherOptions::default()).unwrap();
    let mut buffer: Vec<Matched> = Vec::new();
    compiled.push_matches("rust async", &mut buffer).unwrap();
    assert!(!buffer.is_empty());
    buffer.clear();
    compiled.push_matches("nothing here", &mut buffer).unwrap();
    assert!(buffer.is_empty());
    assert!(buffer.capacity() >= 1, "buffer reuse is preserved");
}

#[test]
fn options_are_serialisable_construction_config() {
    let options = MatcherOptions::default();
    let json = serde_json::to_string(&options).unwrap();
    let back: MatcherOptions = serde_json::from_str(&json).unwrap();
    assert_eq!(options, back);
}

#[test]
fn min_pattern_length_option_is_enforced() {
    let options = MatcherOptions::default().with_min_pattern_length(5);
    let mut builder = MatcherBuilder::new(options);
    let rejected = builder.insert("rust".to_string(), sample_term("rust"));
    assert!(rejected.is_err());
    let accepted = builder.insert("runtime".to_string(), sample_term("runtime"));
    assert!(accepted.is_ok());
}

#[test]
fn case_insensitive_default_matches_reference() {
    let role = RoleName::new("T");
    let thesaurus = thesaurus_from_terms(&role, ["rust"]);
    let compiled = CompiledMatcher::from_thesaurus(&thesaurus, MatcherOptions::default()).unwrap();
    let got = compiled.find_matches("RUST rust Rust", true).unwrap();
    assert_eq!(got.len(), 3);
}

fn sample_term(value: &str) -> NormalizedTerm {
    NormalizedTerm::new(1u64, NormalizedTermValue::from(value))
}

// ---------------------------------------------------------------------------
// Overlap mode and optional word-boundary filter (Refs #79).
// ---------------------------------------------------------------------------

type Span = (String, (usize, usize));

/// Build a matcher from literal patterns, each mapped to itself.
fn matcher_for(patterns: &[&str], options: MatcherOptions) -> CompiledMatcher {
    let mut builder = MatcherBuilder::new(options);
    for (id, pattern) in patterns.iter().enumerate() {
        builder
            .insert(
                (*pattern).to_string(),
                NormalizedTerm::new(id as u64 + 1, NormalizedTermValue::from(*pattern)),
            )
            .unwrap();
    }
    builder.build().unwrap()
}

/// Options for "every occurrence, case-sensitive, no word boundaries".
fn all_occurrences(min_pattern_length: usize) -> MatcherOptions {
    MatcherOptions::default()
        .with_case_insensitive(false)
        .with_min_pattern_length(min_pattern_length)
        .with_overlap(MatchOverlap::All)
        .with_word_boundaries(false)
}

fn spans(matcher: &CompiledMatcher, text: &str) -> Vec<Span> {
    matcher
        .find_matches(text, true)
        .unwrap()
        .into_iter()
        .map(|m| (m.term, m.pos.expect("positions requested")))
        .collect()
}

fn span(term: &str, start: usize, end: usize) -> Span {
    (term.to_string(), (start, end))
}

/// Independent oracle: every case-sensitive occurrence of every pattern,
/// found by testing each char boundary, sorted like `MatchOverlap::All`.
fn naive_all_occurrences(text: &str, patterns: &[&str]) -> Vec<Span> {
    let mut found: Vec<(usize, usize, usize, String)> = Vec::new();
    for (index, pattern) in patterns.iter().enumerate() {
        for start in (0..=text.len()).filter(|&i| text.is_char_boundary(i)) {
            if text[start..].starts_with(pattern) {
                found.push((start, start + pattern.len(), index, (*pattern).to_string()));
            }
        }
    }
    found.sort();
    found
        .into_iter()
        .map(|(start, end, _, term)| (term, (start, end)))
        .collect()
}

#[test]
fn new_options_default_to_previous_behaviour() {
    let options = MatcherOptions::default();
    assert_eq!(options.overlap, MatchOverlap::LeftmostLongest);
    assert!(options.word_boundaries);
    assert_eq!(MatchOverlap::default(), MatchOverlap::LeftmostLongest);
}

/// `MatcherOptions` is `#[non_exhaustive]`, so code outside the crate (this
/// integration test included) can only build it through `Default` and the
/// `with_*` setters. Every field is set away from its default and read back
/// through the public fields.
#[test]
fn options_built_only_via_setters_set_every_field() {
    let defaults = MatcherOptions::default();
    let options = MatcherOptions::default()
        .with_case_insensitive(!defaults.case_insensitive)
        .with_min_pattern_length(defaults.min_pattern_length + 3)
        .with_overlap(MatchOverlap::All)
        .with_word_boundaries(!defaults.word_boundaries);

    assert!(!options.case_insensitive);
    assert_eq!(options.min_pattern_length, DEFAULT_MIN_PATTERN_LENGTH + 3);
    assert_eq!(options.overlap, MatchOverlap::All);
    assert!(!options.word_boundaries);
    assert_ne!(options, defaults);

    // Setters are independent: each sets only its own field, and setting
    // every field back restores the defaults.
    let reset = options
        .clone()
        .with_case_insensitive(true)
        .with_min_pattern_length(DEFAULT_MIN_PATTERN_LENGTH)
        .with_overlap(MatchOverlap::LeftmostLongest)
        .with_word_boundaries(true);
    assert_eq!(reset, defaults);
    let one_changed = options.clone().with_word_boundaries(true);
    assert_eq!(one_changed.overlap, MatchOverlap::All);
    assert!(!one_changed.case_insensitive);
    assert_eq!(
        one_changed.min_pattern_length,
        DEFAULT_MIN_PATTERN_LENGTH + 3
    );

    // `MatchOverlap` is `#[non_exhaustive]` too: an external `match` needs a
    // wildcard arm.
    let label = match options.overlap {
        MatchOverlap::LeftmostLongest => "leftmost-longest",
        MatchOverlap::All => "all",
        _ => "other",
    };
    assert_eq!(label, "all");

    // Options built via setters drive the matcher: case-sensitive, every
    // occurrence, matches inside words.
    let matcher = matcher_for(&["cat"], options.with_min_pattern_length(2));
    assert_eq!(
        spans(&matcher, "concatenate CAT cat"),
        vec![span("cat", 3, 6), span("cat", 16, 19)]
    );
}

#[test]
fn options_without_new_fields_deserialise_with_defaults() {
    let legacy = r#"{"case_insensitive":false,"min_pattern_length":3}"#;
    let options: MatcherOptions = serde_json::from_str(legacy).unwrap();
    assert_eq!(
        options,
        MatcherOptions::default()
            .with_case_insensitive(false)
            .with_min_pattern_length(3)
    );
}

#[test]
fn overlap_mode_serialises_in_snake_case() {
    assert_eq!(
        serde_json::to_string(&MatchOverlap::LeftmostLongest).unwrap(),
        "\"leftmost_longest\""
    );
    assert_eq!(
        serde_json::to_string(&MatchOverlap::All).unwrap(),
        "\"all\""
    );
    let options = all_occurrences(1);
    let json = serde_json::to_string(&options).unwrap();
    assert_eq!(
        serde_json::from_str::<MatcherOptions>(&json).unwrap(),
        options
    );
}

#[test]
fn all_mode_reports_overlapping_occurrences_of_one_pattern() {
    let matcher = matcher_for(&["aa"], all_occurrences(2));
    assert_eq!(
        spans(&matcher, "aaa"),
        vec![span("aa", 0, 2), span("aa", 1, 3)]
    );

    // With word boundaries on, neither occurrence is a whole word.
    let bounded = matcher_for(&["aa"], all_occurrences(2).with_word_boundaries(true));
    assert!(spans(&bounded, "aaa").is_empty());
    assert_eq!(
        spans(&bounded, "aa aaa aa"),
        vec![span("aa", 0, 2), span("aa", 7, 9)]
    );
}

#[test]
fn all_mode_reports_overlapping_occurrences_of_different_patterns() {
    let patterns = ["abc", "bcd"];
    let standalone = vec![span("abc", 0, 3), span("bcd", 4, 7)];

    // Separate words: both reported in every mode.
    for word_boundaries in [true, false] {
        for overlap in [MatchOverlap::LeftmostLongest, MatchOverlap::All] {
            let matcher = matcher_for(
                &patterns,
                MatcherOptions::default()
                    .with_overlap(overlap)
                    .with_word_boundaries(word_boundaries),
            );
            assert_eq!(
                spans(&matcher, "abc bcd"),
                standalone,
                "overlap={overlap:?} word_boundaries={word_boundaries}"
            );
        }
    }

    // Overlapping inside one word, no boundary filter: `All` reports both.
    let all = matcher_for(&patterns, all_occurrences(2));
    assert_eq!(
        spans(&all, "abcd"),
        vec![span("abc", 0, 3), span("bcd", 1, 4)]
    );

    // Leftmost-longest without the boundary filter still reports one match
    // per position.
    let leftmost_anywhere = matcher_for(
        &patterns,
        MatcherOptions::default().with_word_boundaries(false),
    );
    assert_eq!(spans(&leftmost_anywhere, "abcd"), vec![span("abc", 0, 3)]);

    // `All` with the boundary filter rejects both inside `abcd`.
    let all_bounded = matcher_for(
        &patterns,
        MatcherOptions::default().with_overlap(MatchOverlap::All),
    );
    assert!(spans(&all_bounded, "abcd").is_empty());
}

#[test]
fn all_mode_reports_short_pattern_inside_longer_occurrence() {
    let patterns = ["cat", "the cat sat"];
    let text = "the cat sat";

    let all = matcher_for(
        &patterns,
        MatcherOptions::default().with_overlap(MatchOverlap::All),
    );
    assert_eq!(
        spans(&all, text),
        vec![span("the cat sat", 0, 11), span("cat", 4, 7)]
    );

    // The default mode keeps only the longest match.
    let default = matcher_for(&patterns, MatcherOptions::default());
    assert_eq!(spans(&default, text), vec![span("the cat sat", 0, 11)]);
}

#[test]
fn all_mode_respects_case_sensitivity() {
    let text = "Rust rust RUST";
    let sensitive = matcher_for(
        &["Rust"],
        MatcherOptions::default()
            .with_case_insensitive(false)
            .with_overlap(MatchOverlap::All),
    );
    assert_eq!(spans(&sensitive, text), vec![span("Rust", 0, 4)]);

    let insensitive = matcher_for(
        &["Rust"],
        MatcherOptions::default()
            .with_case_insensitive(true)
            .with_overlap(MatchOverlap::All),
    );
    assert_eq!(
        spans(&insensitive, text),
        vec![span("Rust", 0, 4), span("Rust", 5, 9), span("Rust", 10, 14)]
    );
}

#[test]
fn all_mode_single_character_patterns_with_and_without_boundaries() {
    let patterns = ["a", "I"];
    let text = "I saw a cat and a bat";

    let anywhere = matcher_for(&patterns, all_occurrences(1));
    let got = spans(&anywhere, text);
    assert_eq!(got, naive_all_occurrences(text, &patterns));
    assert_eq!(
        got,
        vec![
            span("I", 0, 1),
            span("a", 3, 4),
            span("a", 6, 7),
            span("a", 9, 10),
            span("a", 12, 13),
            span("a", 16, 17),
            span("a", 19, 20),
        ]
    );

    let words = matcher_for(&patterns, all_occurrences(1).with_word_boundaries(true));
    assert_eq!(
        spans(&words, text),
        vec![span("I", 0, 1), span("a", 6, 7), span("a", 16, 17)]
    );
}

#[test]
fn all_mode_multibyte_text_gives_char_boundary_offsets() {
    let text = "h\u{e9}llo w\u{f6}rld \u{6771}\u{4eac}\u{30bf}\u{30ef}\u{30fc} \u{1d11e}x\u{1d11e} \u{e9}\u{e9}\u{e9}";
    let patterns = [
        "llo",
        "w\u{f6}rld",
        "\u{6771}\u{4eac}",
        "\u{4eac}\u{30bf}",
        "\u{1d11e}",
        "\u{e9}\u{e9}",
    ];
    let matcher = matcher_for(&patterns, all_occurrences(1));
    let got = spans(&matcher, text);

    assert_eq!(got, naive_all_occurrences(text, &patterns));
    for (term, (start, end)) in &got {
        assert!(text.is_char_boundary(*start) && text.is_char_boundary(*end));
        assert_eq!(&text[*start..*end], term.as_str());
    }
    // Hand-checked offsets: `é` is 2 bytes, CJK 3 bytes, U+1D11E 4 bytes.
    assert_eq!(
        got,
        vec![
            span("llo", 3, 6),
            span("w\u{f6}rld", 7, 13),
            span("\u{6771}\u{4eac}", 14, 20),
            span("\u{4eac}\u{30bf}", 17, 23),
            span("\u{1d11e}", 30, 34),
            span("\u{1d11e}", 35, 39),
            span("\u{e9}\u{e9}", 40, 44),
            span("\u{e9}\u{e9}", 42, 46),
        ]
    );

    // With word boundaries on, the crate's rule applies: only ASCII
    // alphanumerics and `_` continue a word, so non-ASCII neighbours (`é`, CJK)
    // count as boundaries and those matches survive, while both U+1D11E
    // occurrences touch the ASCII `x` and are rejected.
    let words = matcher_for(&patterns, all_occurrences(1).with_word_boundaries(true));
    let expected: Vec<Span> = got
        .iter()
        .filter(|(term, _)| term != "\u{1d11e}")
        .cloned()
        .collect();
    assert_eq!(spans(&words, text), expected);
}

#[test]
fn all_mode_agrees_with_naive_oracle_on_mixed_text() {
    let patterns = ["an", "ana", "nan", "banana", "na"];
    let text = "banana bandana ananas nanana";
    let matcher = matcher_for(&patterns, all_occurrences(2));
    assert_eq!(
        spans(&matcher, text),
        naive_all_occurrences(text, &patterns)
    );
}

#[test]
fn all_mode_find_iter_and_push_matches_agree_with_find_matches() {
    let patterns = ["an", "ana", "nan", "banana"];
    let text = "banana ananas";
    let matcher = matcher_for(&patterns, all_occurrences(2));

    let without_positions: Vec<String> = matcher
        .find_matches(text, false)
        .unwrap()
        .into_iter()
        .map(|m| m.term)
        .collect();
    let via_iter: Vec<String> = matcher.find_iter(text).map(|m| m.term).collect();
    let mut buffer = Vec::new();
    matcher.push_matches(text, &mut buffer).unwrap();
    let via_push: Vec<String> = buffer.into_iter().map(|m| m.term).collect();

    let expected: Vec<String> = naive_all_occurrences(text, &patterns)
        .into_iter()
        .map(|(term, _)| term)
        .collect();
    assert_eq!(without_positions, expected);
    assert_eq!(via_iter, expected);
    assert_eq!(via_push, expected);
}

#[test]
fn default_mode_without_word_boundaries_matches_inside_words() {
    let matcher = matcher_for(
        &["cat"],
        MatcherOptions::default().with_word_boundaries(false),
    );
    assert_eq!(
        spans(&matcher, "concatenate a cat"),
        vec![span("cat", 3, 6), span("cat", 14, 17)]
    );
    let via_iter: Vec<String> = matcher
        .find_iter("concatenate a cat")
        .map(|m| m.term)
        .collect();
    assert_eq!(via_iter, vec!["cat".to_string(), "cat".to_string()]);
}

#[test]
fn explicit_default_options_still_equal_reference_find_matches() {
    let thesaurus = thesaurus_sample();
    let explicit = MatcherOptions::default()
        .with_case_insensitive(true)
        .with_min_pattern_length(2)
        .with_overlap(MatchOverlap::LeftmostLongest)
        .with_word_boundaries(true);
    assert_eq!(explicit, MatcherOptions::default());
    let compiled = CompiledMatcher::from_thesaurus(&thesaurus, explicit).unwrap();

    let text = "Rust async runtimes: rust, RUST ASYNC, knowledge graphs and a \
        knowledge graph; machine learning rustacean async runtime.";
    let expected = find_matches(text, &thesaurus, true).unwrap();
    assert_eq!(compiled.find_matches(text, true).unwrap(), expected);

    let expected_terms: Vec<String> = expected.into_iter().map(|m| m.term).collect();
    let via_iter: Vec<String> = compiled.find_iter(text).map(|m| m.term).collect();
    assert_eq!(via_iter, expected_terms);
    let mut buffer = Vec::new();
    compiled.push_matches(text, &mut buffer).unwrap();
    let via_push: Vec<String> = buffer.into_iter().map(|m| m.term).collect();
    assert_eq!(via_push, expected_terms);
}

// ---------------------------------------------------------------------------
// Whitespace-only patterns (Refs #86).
// ---------------------------------------------------------------------------

#[test]
fn blank_patterns_are_rejected_by_default() {
    let options = MatcherOptions::default();
    assert!(!options.allow_blank_patterns);
    let mut builder = MatcherBuilder::new(options);
    for blank in ["  ", "\t\t", " \n "] {
        let err = builder
            .insert(blank.to_string(), sample_term("gap"))
            .unwrap_err();
        assert!(
            err.to_string().contains("pattern is blank"),
            "{blank:?}: {err}"
        );
    }
    let err = builder
        .insert(String::new(), sample_term("gap"))
        .unwrap_err();
    assert!(err.to_string().contains("pattern is blank"), "{err}");
}

#[test]
fn allow_blank_patterns_accepts_whitespace_but_never_empty() {
    let mut builder = MatcherBuilder::new(
        MatcherOptions::default()
            .with_allow_blank_patterns(true)
            .with_min_pattern_length(0),
    );
    builder
        .insert("  ".to_string(), sample_term("gap"))
        .unwrap();
    builder
        .insert("\t".to_string(), sample_term("tab"))
        .unwrap();
    let err = builder
        .insert(String::new(), sample_term("empty"))
        .unwrap_err();
    assert!(err.to_string().contains("pattern is empty"), "{err}");
    // The minimum length still applies to blank patterns.
    let mut strict = MatcherBuilder::new(MatcherOptions::default().with_allow_blank_patterns(true));
    assert!(strict.insert(" ".to_string(), sample_term("one")).is_err());
    // Duplicates are still rejected.
    assert!(
        builder
            .insert("  ".to_string(), sample_term("gap"))
            .is_err()
    );
}

#[test]
fn allow_blank_patterns_finds_overlapping_whitespace_runs() {
    let options = MatcherOptions::default()
        .with_allow_blank_patterns(true)
        .with_overlap(MatchOverlap::All);
    let matcher = matcher_for(&["  "], options);
    // Spaces are not word-continuation characters, so the default
    // word-boundary filter keeps every occurrence.
    assert_eq!(
        spans(&matcher, "    "),
        vec![span("  ", 0, 2), span("  ", 1, 3), span("  ", 2, 4)]
    );

    let leftmost = matcher_for(
        &["  "],
        MatcherOptions::default().with_allow_blank_patterns(true),
    );
    assert_eq!(
        spans(&leftmost, "    "),
        vec![span("  ", 0, 2), span("  ", 2, 4)]
    );
}

#[test]
fn allow_blank_patterns_leaves_other_matching_unchanged() {
    let text = "Rust and RUST ASYNC power the knowledge graph with an async runtime.";
    let thesaurus = thesaurus_sample();
    let default = CompiledMatcher::from_thesaurus(&thesaurus, MatcherOptions::default()).unwrap();
    let allowing = CompiledMatcher::from_thesaurus(
        &thesaurus,
        MatcherOptions::default().with_allow_blank_patterns(true),
    )
    .unwrap();
    assert_eq!(
        allowing.find_matches(text, true).unwrap(),
        default.find_matches(text, true).unwrap()
    );
}

#[test]
fn allow_blank_patterns_serialisation() {
    let json = serde_json::to_value(MatcherOptions::default()).unwrap();
    assert_eq!(json["allow_blank_patterns"], false);
    let json =
        serde_json::to_value(MatcherOptions::default().with_allow_blank_patterns(true)).unwrap();
    assert_eq!(json["allow_blank_patterns"], true);
    let back: MatcherOptions = serde_json::from_value(json).unwrap();
    assert!(back.allow_blank_patterns);
}

#[test]
fn blank_patterns_respect_the_word_boundary_filter() {
    let bounded = matcher_for(
        &["  "],
        MatcherOptions::default().with_allow_blank_patterns(true),
    );
    assert!(spans(&bounded, "a  b").is_empty());
    assert_eq!(spans(&bounded, "a,  b"), Vec::<Span>::new());
    assert_eq!(spans(&bounded, "-  -"), vec![span("  ", 1, 3)]);

    let anywhere = matcher_for(
        &["  "],
        MatcherOptions::default()
            .with_allow_blank_patterns(true)
            .with_word_boundaries(false),
    );
    assert_eq!(spans(&anywhere, "a  b"), vec![span("  ", 1, 3)]);
}

/// Every option field is always written, so non-self-describing formats
/// (bincode serialises a struct as a fixed field sequence) round-trip.
#[test]
fn options_round_trip_through_bincode() {
    for options in [
        MatcherOptions::default(),
        MatcherOptions::default()
            .with_allow_blank_patterns(true)
            .with_overlap(MatchOverlap::All)
            .with_word_boundaries(false)
            .with_case_insensitive(false)
            .with_min_pattern_length(1),
    ] {
        let bytes = bincode::serialize(&options).unwrap();
        let back: MatcherOptions = bincode::deserialize(&bytes).unwrap();
        assert_eq!(back, options);
    }
}
