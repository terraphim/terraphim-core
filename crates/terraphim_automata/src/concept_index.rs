//! Concept-id -> synonyms index, and "alternatives at a position" lookup.
//!
//! A [`Thesaurus`] maps each synonym to the concept it normalises to, so every
//! synonym of a concept shares that concept's id. [`ConceptIndex`] inverts the
//! map once per thesaurus load, giving the full synonym list for a concept id.
//! Editors use it to offer "the other words for this concept" as alternatives
//! for a matched span: [`alternatives_at`] combines the index with a
//! [`CompiledMatcher`] to answer that for a cursor offset.
//!
//! The index is built from the final [`Thesaurus`] state. Because a thesaurus
//! is keyed by synonym, a synonym string belongs to exactly one concept: when
//! a builder inserts the same synonym for two concepts, the later insert
//! overwrites the earlier one before the index ever sees it.

use std::ops::Range;

use ahash::AHashMap;
use terraphim_types::{NormalizedTermValue, Thesaurus};

use crate::Result;
use crate::compiled::CompiledMatcher;
use crate::matcher::Matched;

/// A borrowed thesaurus entry: (synonym, concept name, name display form).
type ThesaurusEntry<'a> = (
    &'a NormalizedTermValue,
    &'a NormalizedTermValue,
    Option<&'a str>,
);

/// One concept's terms plus the display form of its name.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Concept {
    /// The concept name at index 0, then the remaining synonyms sorted
    /// ascending. Never empty.
    terms: Vec<NormalizedTermValue>,
    /// Original-case form of the name (`NormalizedTerm::display_value`), if
    /// the thesaurus recorded one.
    display: Option<String>,
    /// Every distinct name the thesaurus gave this id, sorted. Usually one;
    /// more only for an inconsistent thesaurus. Used to check that a match
    /// came from the thesaurus this index was built from.
    names: Vec<NormalizedTermValue>,
}

/// Inverted view of a [`Thesaurus`]: concept id -> every term of that concept.
///
/// Build it once per thesaurus load with [`ConceptIndex::from_thesaurus`] and
/// reuse it for every lookup.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConceptIndex {
    concepts: AHashMap<u64, Concept>,
}

impl ConceptIndex {
    /// Build the index in one pass over `thesaurus`.
    ///
    /// Ordering is deterministic regardless of the thesaurus' hash order: the
    /// concept name comes first, followed by the other synonyms sorted
    /// ascending. If entries sharing an id disagree on the concept name (an
    /// inconsistent thesaurus), the lexicographically smallest name is used,
    /// and likewise the smallest display form recorded for that name.
    pub fn from_thesaurus(thesaurus: &Thesaurus) -> Self {
        // id -> every (synonym, name, display) entry, gathered in hash order
        // and resolved deterministically below.
        let mut grouped: AHashMap<u64, Vec<ThesaurusEntry<'_>>> = AHashMap::new();
        for (synonym, term) in thesaurus {
            grouped.entry(term.id).or_default().push((
                synonym,
                &term.value,
                term.display_value.as_deref(),
            ));
        }

        let concepts = grouped
            .into_iter()
            .filter_map(|(id, entries)| {
                // Every group holds at least one entry, so `min` is `Some`.
                let name = entries.iter().map(|(_, name, _)| *name).min()?;
                let display = entries
                    .iter()
                    .filter(|(_, entry_name, _)| *entry_name == name)
                    .filter_map(|(_, _, display)| *display)
                    .min()
                    .map(str::to_owned);
                let mut synonyms: Vec<NormalizedTermValue> = entries
                    .iter()
                    .map(|(synonym, _, _)| *synonym)
                    .filter(|synonym| *synonym != name)
                    .cloned()
                    .collect();
                synonyms.sort_unstable();
                synonyms.dedup();
                let mut names: Vec<NormalizedTermValue> =
                    entries.iter().map(|(_, name, _)| (*name).clone()).collect();
                names.sort_unstable();
                names.dedup();
                let mut terms = Vec::with_capacity(synonyms.len() + 1);
                terms.push(name.clone());
                terms.extend(synonyms);
                Some((
                    id,
                    Concept {
                        terms,
                        display,
                        names,
                    },
                ))
            })
            .collect();

        Self { concepts }
    }

    /// Every term of concept `id`: the concept name first, then the other
    /// synonyms sorted ascending. Empty when `id` is unknown.
    ///
    /// Terms are normalised (trimmed, lowercase). Use
    /// [`ConceptIndex::display_name`] or [`Alternatives::alternatives`] for
    /// replacement text.
    pub fn synonyms_of(&self, id: u64) -> &[NormalizedTermValue] {
        self.concepts
            .get(&id)
            .map_or(&[], |concept| concept.terms.as_slice())
    }

    /// The normalised concept name for `id`, if the concept is known.
    pub fn concept_name(&self, id: u64) -> Option<&NormalizedTermValue> {
        self.concepts
            .get(&id)
            .and_then(|concept| concept.terms.first())
    }

    /// The concept name for `id` in its original case
    /// (`NormalizedTerm::display_value`), falling back to the normalised
    /// name when the thesaurus recorded none.
    pub fn display_name(&self, id: u64) -> Option<&str> {
        let concept = self.concepts.get(&id)?;
        concept
            .display
            .as_deref()
            .or_else(|| concept.terms.first().map(NormalizedTermValue::as_str))
    }

    /// Whether the thesaurus gave concept `id` the name `name`.
    fn has_name(&self, id: u64, name: &NormalizedTermValue) -> bool {
        self.concepts
            .get(&id)
            .is_some_and(|concept| concept.names.binary_search(name).is_ok())
    }

    /// Number of concepts in the index.
    pub fn len(&self) -> usize {
        self.concepts.len()
    }

    /// Whether the index holds no concepts.
    pub fn is_empty(&self) -> bool {
        self.concepts.is_empty()
    }
}

/// The alternatives for the concept matched at a text position.
///
/// Non-exhaustive so fields can be added without a breaking release; it is
/// only ever constructed by [`alternatives_in`] and [`alternatives_at`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Alternatives {
    /// Byte range of the matched span in the searched text.
    pub range: Range<usize>,
    /// Id of the matched concept.
    pub concept_id: u64,
    /// The thesaurus key that matched (normalised form of the span).
    pub matched: NormalizedTermValue,
    /// Replacement text for each other term of the concept, in
    /// [`ConceptIndex::synonyms_of`] order with the matched term removed.
    /// The concept name uses its display form ([`ConceptIndex::display_name`]),
    /// so acronyms and proper nouns keep their case; synonyms are normalised
    /// because a thesaurus records no per-synonym display form. Empty for a
    /// single-term concept.
    pub alternatives: Vec<String>,
}

/// Select the match covering byte `offset` from `matches` and list its
/// alternatives.
///
/// `matches` must be positioned and sorted by start, as returned by
/// [`CompiledMatcher::find_matches`] or [`crate::find_matches`] with
/// `return_positions = true`; matches without a position are ignored. They
/// must be default-mode (non-overlapping) matches: a matcher built with
/// [`crate::MatchOverlap::All`] reports overlapping occurrences, which break
/// the "matches never overlap" assumption below and make the selected match
/// unspecified. Use this
/// to reuse matches cached per edit instead of rescanning the text on every
/// cursor move.
///
/// `offset` and the returned range are **byte** offsets; LSP and browser
/// consumers must convert from and to their UTF-16 positions. A match covers
/// `offset` when `start <= offset <= end`, so a cursor placed just after a
/// word still finds it. Matches never overlap, but two can touch (`end` of one
/// equal to `start` of the next, in scripts without word separators); the
/// match that starts at `offset` then wins.
///
/// The matcher and `index` must be built from the same thesaurus. A match
/// whose key is not a term of its concept in `index`, or whose concept name
/// differs from the one `index` recorded for that id (a stale or cross-wired
/// pair), gives `None` instead of another concept's terms. This catches
/// mismatched concepts, not every stale thesaurus: an index whose concept
/// keeps the same id, name and matched key but gained or lost other synonyms
/// is accepted. Patterns the matcher
/// rejects as too short can still be listed as alternatives but are never
/// matched themselves.
pub fn alternatives_in(
    matches: &[Matched],
    offset: usize,
    index: &ConceptIndex,
) -> Option<Alternatives> {
    let mut found = None;
    for matched in matches {
        let Some((start, end)) = matched.pos else {
            continue;
        };
        if start > offset {
            break;
        }
        if offset <= end {
            // Keep scanning: a match starting exactly at `offset` beats one
            // that ends there.
            found = Some((start..end, matched));
        }
    }

    let (range, matched) = found?;
    let concept_id = matched.normalized_term.id;
    let matched_key = NormalizedTermValue::new(matched.term.clone());
    let terms = index.synonyms_of(concept_id);
    // A match from the thesaurus this index was built from always has its key
    // among the concept's terms and its concept name among the names recorded
    // for that id. Otherwise the matches and the index are a stale or
    // cross-wired pair: refuse rather than offer another concept's terms (or
    // a misleading empty list).
    if !terms.contains(&matched_key) || !index.has_name(concept_id, &matched.normalized_term.value)
    {
        return None;
    }
    Some({
        let alternatives = terms
            .iter()
            .enumerate()
            .filter(|(_, term)| **term != matched_key)
            .map(
                |(position, term)| match (position, index.display_name(concept_id)) {
                    (0, Some(display)) => display.to_owned(),
                    _ => term.as_str().to_owned(),
                },
            )
            .collect();
        Alternatives {
            range,
            concept_id,
            matched: matched_key,
            alternatives,
        }
    })
}

/// Scan `text` with `matcher` and return the alternatives at byte `offset`.
///
/// Convenience wrapper over [`alternatives_in`], with the same offset rules.
/// `matcher` must use the default [`crate::MatchOverlap::LeftmostLongest`]
/// overlap mode, for the reason given on [`alternatives_in`].
/// It scans the whole of `text` on every call; callers that look up many
/// offsets in the same text should cache `matcher.find_matches(text, true)`
/// and call [`alternatives_in`]. An `offset` beyond the text gives `Ok(None)`.
pub fn alternatives_at(
    text: &str,
    offset: usize,
    matcher: &CompiledMatcher,
    index: &ConceptIndex,
) -> Result<Option<Alternatives>> {
    if offset > text.len() {
        return Ok(None);
    }
    Ok(alternatives_in(
        &matcher.find_matches(text, true)?,
        offset,
        index,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiled::MatcherOptions;
    use terraphim_types::NormalizedTerm;

    fn value(term: &str) -> NormalizedTermValue {
        NormalizedTermValue::from(term)
    }

    fn values(terms: &[&str]) -> Vec<NormalizedTermValue> {
        terms.iter().map(|term| value(term)).collect()
    }

    fn strings(terms: &[&str]) -> Vec<String> {
        terms.iter().map(|term| term.to_string()).collect()
    }

    /// Insert `synonyms` (plus the concept name itself) for concept `id`.
    fn add_concept(thesaurus: &mut Thesaurus, id: u64, name: &str, synonyms: &[&str]) {
        let term = NormalizedTerm::new(id, value(name));
        thesaurus.insert(value(name), term.clone());
        for synonym in synonyms {
            thesaurus.insert(value(synonym), term.clone());
        }
    }

    fn fixture() -> Thesaurus {
        let mut thesaurus = Thesaurus::new("fixture".to_string());
        add_concept(
            &mut thesaurus,
            1,
            "decision",
            &["judgment", "choice", "trade-off", "bet"],
        );
        add_concept(&mut thesaurus, 2, "eraser", &[]);
        add_concept(&mut thesaurus, 3, "paperclip", &["paper clip", "clip"]);
        thesaurus
    }

    fn matcher_for(thesaurus: &Thesaurus) -> CompiledMatcher {
        CompiledMatcher::from_thesaurus(thesaurus, MatcherOptions::default()).unwrap()
    }

    #[test]
    fn multi_synonym_concept_lists_name_first_then_sorted() {
        let index = ConceptIndex::from_thesaurus(&fixture());
        assert_eq!(
            index.synonyms_of(1),
            values(&["decision", "bet", "choice", "judgment", "trade-off"]).as_slice()
        );
        assert_eq!(index.concept_name(1), Some(&value("decision")));
        assert_eq!(index.len(), 3);
    }

    #[test]
    fn single_term_concept_has_only_its_name() {
        let index = ConceptIndex::from_thesaurus(&fixture());
        assert_eq!(index.synonyms_of(2), values(&["eraser"]).as_slice());
    }

    #[test]
    fn unknown_id_is_empty() {
        let index = ConceptIndex::from_thesaurus(&fixture());
        assert!(index.synonyms_of(99).is_empty());
        assert_eq!(index.concept_name(99), None);
        assert!(ConceptIndex::from_thesaurus(&Thesaurus::new("empty".into())).is_empty());
    }

    #[test]
    fn concept_name_missing_as_key_is_still_listed_first() {
        let mut thesaurus = Thesaurus::new("no-name-key".to_string());
        let term = NormalizedTerm::new(7, value("vehicle"));
        thesaurus.insert(value("car"), term.clone());
        thesaurus.insert(value("automobile"), term);
        let index = ConceptIndex::from_thesaurus(&thesaurus);
        assert_eq!(
            index.synonyms_of(7),
            values(&["vehicle", "automobile", "car"]).as_slice()
        );
    }

    /// A thesaurus is keyed by synonym, so inserting a synonym for a second
    /// concept moves it: the index reflects the final state only.
    #[test]
    fn shared_synonym_belongs_to_the_last_insert() {
        let mut thesaurus = fixture();
        thesaurus.insert(value("clip"), NormalizedTerm::new(4, value("video")));
        let index = ConceptIndex::from_thesaurus(&thesaurus);
        assert_eq!(
            index.synonyms_of(3),
            values(&["paperclip", "paper clip"]).as_slice()
        );
        assert_eq!(index.synonyms_of(4), values(&["video", "clip"]).as_slice());
    }

    #[test]
    fn inconsistent_names_resolve_to_smallest() {
        let mut thesaurus = Thesaurus::new("inconsistent".to_string());
        thesaurus.insert(value("zeta"), NormalizedTerm::new(5, value("omega")));
        thesaurus.insert(value("alpha"), NormalizedTerm::new(5, value("beta")));
        let index = ConceptIndex::from_thesaurus(&thesaurus);
        assert_eq!(index.concept_name(5), Some(&value("beta")));
        assert_eq!(
            index.synonyms_of(5),
            values(&["beta", "alpha", "zeta"]).as_slice()
        );
    }

    #[test]
    fn index_is_independent_of_insertion_order() {
        let forward = fixture();
        let mut reversed = Thesaurus::new("reversed".to_string());
        add_concept(&mut reversed, 3, "paperclip", &["clip", "paper clip"]);
        add_concept(&mut reversed, 2, "eraser", &[]);
        add_concept(
            &mut reversed,
            1,
            "decision",
            &["bet", "trade-off", "choice", "judgment"],
        );
        assert_eq!(
            ConceptIndex::from_thesaurus(&forward),
            ConceptIndex::from_thesaurus(&reversed)
        );
    }

    #[test]
    fn alternatives_at_excludes_the_matched_term() {
        let thesaurus = fixture();
        let (matcher, index) = (
            matcher_for(&thesaurus),
            ConceptIndex::from_thesaurus(&thesaurus),
        );
        let text = "Every Choice is a bet.";
        let found = alternatives_at(text, 8, &matcher, &index).unwrap().unwrap();
        assert_eq!(found.range, 6..12);
        assert_eq!(&text[found.range.clone()], "Choice");
        assert_eq!(found.concept_id, 1);
        assert_eq!(found.matched, value("choice"));
        assert_eq!(
            found.alternatives,
            strings(&["decision", "bet", "judgment", "trade-off"])
        );
    }

    #[test]
    fn alternatives_at_covers_span_edges() {
        let thesaurus = fixture();
        let (matcher, index) = (
            matcher_for(&thesaurus),
            ConceptIndex::from_thesaurus(&thesaurus),
        );
        let text = "a paper clip here";
        for offset in [2, 7, 12] {
            let found = alternatives_at(text, offset, &matcher, &index)
                .unwrap()
                .unwrap_or_else(|| panic!("offset {offset} should hit"));
            assert_eq!(found.range, 2..12, "offset {offset}");
            assert_eq!(found.alternatives, strings(&["paperclip", "clip"]));
        }
        assert_eq!(alternatives_at(text, 1, &matcher, &index).unwrap(), None);
        assert_eq!(alternatives_at(text, 14, &matcher, &index).unwrap(), None);
    }

    #[test]
    fn alternatives_at_single_term_concept_is_empty_list() {
        let thesaurus = fixture();
        let (matcher, index) = (
            matcher_for(&thesaurus),
            ConceptIndex::from_thesaurus(&thesaurus),
        );
        let found = alternatives_at("an eraser", 4, &matcher, &index)
            .unwrap()
            .unwrap();
        assert_eq!(found.concept_id, 2);
        assert!(found.alternatives.is_empty());
    }

    #[test]
    fn alternatives_at_out_of_range_offset_is_none() {
        let thesaurus = fixture();
        let (matcher, index) = (
            matcher_for(&thesaurus),
            ConceptIndex::from_thesaurus(&thesaurus),
        );
        assert_eq!(alternatives_at("bet", 4, &matcher, &index).unwrap(), None);
        assert_eq!(alternatives_at("", 0, &matcher, &index).unwrap(), None);
    }

    #[test]
    fn alternatives_at_prefers_match_starting_at_offset() {
        // CJK has no word separators, so two matches can touch.
        let mut thesaurus = Thesaurus::new("cjk".to_string());
        add_concept(&mut thesaurus, 10, "日本", &[]);
        add_concept(&mut thesaurus, 11, "東京", &["首都"]);
        let (matcher, index) = (
            matcher_for(&thesaurus),
            ConceptIndex::from_thesaurus(&thesaurus),
        );
        let text = "日本東京";
        let boundary = "日本".len();
        let found = alternatives_at(text, boundary, &matcher, &index)
            .unwrap()
            .unwrap();
        assert_eq!(found.concept_id, 11);
        assert_eq!(found.range, boundary..text.len());
        assert_eq!(found.alternatives, strings(&["首都"]));
    }

    #[test]
    fn display_name_keeps_original_case() {
        let mut thesaurus = Thesaurus::new("display".to_string());
        let llm = NormalizedTerm::new(20, value("llm")).with_display_value("LLM".to_string());
        thesaurus.insert(value("llm"), llm.clone());
        thesaurus.insert(value("large language model"), llm);
        add_concept(&mut thesaurus, 21, "model", &[]);
        let index = ConceptIndex::from_thesaurus(&thesaurus);
        assert_eq!(index.display_name(20), Some("LLM"));
        assert_eq!(index.display_name(21), Some("model"));
        assert_eq!(index.display_name(99), None);

        let matcher = matcher_for(&thesaurus);
        let found = alternatives_at("a large language model", 4, &matcher, &index)
            .unwrap()
            .unwrap();
        assert_eq!(found.range, 2..22);
        assert_eq!(found.alternatives, strings(&["LLM"]));
    }

    #[test]
    fn display_name_resolution_is_deterministic() {
        let mut thesaurus = Thesaurus::new("display-conflict".to_string());
        thesaurus.insert(
            value("rust lang"),
            NormalizedTerm::new(30, value("rust")).with_display_value("Rust".to_string()),
        );
        thesaurus.insert(
            value("rustlang"),
            NormalizedTerm::new(30, value("rust")).with_display_value("RUST".to_string()),
        );
        thesaurus.insert(value("rust"), NormalizedTerm::new(30, value("rust")));
        let index = ConceptIndex::from_thesaurus(&thesaurus);
        // Smallest recorded display form wins ("RUST" < "Rust"); a missing
        // display value on another entry does not erase it.
        assert_eq!(index.display_name(30), Some("RUST"));
    }

    #[test]
    fn alternatives_in_reuses_cached_matches() {
        let thesaurus = fixture();
        let (matcher, index) = (
            matcher_for(&thesaurus),
            ConceptIndex::from_thesaurus(&thesaurus),
        );
        let text = "Every Choice is a bet.";
        let cached = matcher.find_matches(text, true).unwrap();
        let at_choice = alternatives_in(&cached, 8, &index).unwrap();
        assert_eq!(at_choice.range, 6..12);
        let at_bet = alternatives_in(&cached, 19, &index).unwrap();
        assert_eq!(&text[at_bet.range.clone()], "bet");
        assert_eq!(
            at_bet.alternatives,
            strings(&["decision", "choice", "judgment", "trade-off"])
        );
        assert_eq!(alternatives_in(&cached, 14, &index), None);
        assert_eq!(
            alternatives_at(text, 8, &matcher, &index).unwrap(),
            Some(at_choice)
        );
    }

    #[test]
    fn alternatives_in_ignores_unpositioned_matches() {
        let thesaurus = fixture();
        let (matcher, index) = (
            matcher_for(&thesaurus),
            ConceptIndex::from_thesaurus(&thesaurus),
        );
        let unpositioned = matcher.find_matches("a bet", false).unwrap();
        assert!(!unpositioned.is_empty());
        assert_eq!(alternatives_in(&unpositioned, 3, &index), None);
    }

    #[test]
    fn alternatives_in_rejects_mismatched_index() {
        let thesaurus = fixture();
        let matcher = matcher_for(&thesaurus);
        let cached = matcher.find_matches("a bet", true).unwrap();

        // Same id (1), different concept: must not leak its terms.
        let mut other = Thesaurus::new("other".to_string());
        add_concept(&mut other, 1, "colour", &["hue"]);
        assert_eq!(
            alternatives_in(&cached, 3, &ConceptIndex::from_thesaurus(&other)),
            None
        );
        // Unknown id: None, not an empty alternatives list.
        let empty = ConceptIndex::from_thesaurus(&Thesaurus::new("empty".into()));
        assert_eq!(alternatives_in(&cached, 3, &empty), None);
    }

    #[test]
    fn alternatives_in_rejects_same_id_and_key_from_other_concept() {
        let thesaurus = fixture();
        let matcher = matcher_for(&thesaurus);
        // "bet" matches concept 1 ("decision").
        let cached = matcher.find_matches("a bet", true).unwrap();

        // Another thesaurus reuses id 1 and the key "bet" for a different
        // concept ("wager"): same id, key present, wrong concept.
        let mut other = Thesaurus::new("other".to_string());
        add_concept(&mut other, 1, "wager", &["bet", "stake"]);
        assert_eq!(
            alternatives_in(&cached, 3, &ConceptIndex::from_thesaurus(&other)),
            None
        );
    }

    #[test]
    fn alternatives_in_accepts_any_name_of_inconsistent_concept() {
        // Entries for id 5 disagree on the name; the index picks "beta", but
        // a match carrying "omega" is still from this thesaurus.
        let mut thesaurus = Thesaurus::new("inconsistent".to_string());
        thesaurus.insert(value("zeta"), NormalizedTerm::new(5, value("omega")));
        thesaurus.insert(value("alpha"), NormalizedTerm::new(5, value("beta")));
        let (matcher, index) = (
            matcher_for(&thesaurus),
            ConceptIndex::from_thesaurus(&thesaurus),
        );
        let found = alternatives_at("zeta", 1, &matcher, &index)
            .unwrap()
            .unwrap();
        assert_eq!(found.concept_id, 5);
        assert_eq!(found.alternatives, strings(&["beta", "alpha"]));
    }
}
