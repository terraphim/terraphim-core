//! Compile-once, match-many matcher API.
//!
//! This module exposes a first-class boundary between *pattern
//! validation/compilation* and *repeated matching*:
//!
//! - [`MatcherOptions`] is the explicit, serialisable construction
//!   configuration.
//! - [`MatcherBuilder`] validates patterns up front and rejects invalid
//!   (empty, blank unless [`MatcherOptions::allow_blank_patterns`] is set, or
//!   too-short) and duplicate patterns.
//! - [`CompiledMatcher`] is an immutable, `Send + Sync` matcher that can be
//!   reused across many queries and threads without rebuilding the automaton.
//!
//! Compiled internals are intentionally **not** serialisable; persist the
//! source patterns plus [`MatcherOptions`] instead and rebuild on load.
//!
//! With default options, results are identical to
//! [`crate::matcher::find_matches`] semantics (leftmost-longest, whole-word
//! matches only, ASCII case-insensitive, minimum pattern length enforced).
//! [`MatcherOptions::overlap`] and [`MatcherOptions::word_boundaries`] opt in
//! to reporting every (possibly overlapping) occurrence and to matching
//! inside words, for callers such as anchor re-location that need a plain
//! "find all occurrences" search.
//!
//! # Positions without per-match allocation
//!
//! [`CompiledMatcher::find_matches`] returns owned [`Matched`] records, which
//! clone the pattern string and its [`NormalizedTerm`] for every match.
//! Callers that only need *where* each match is can use
//! [`CompiledMatcher::find_positions`] (a borrowing, allocation-free
//! iterator) or [`CompiledMatcher::push_positions`] (appends to a reusable
//! buffer, sorted for [`MatchOverlap::All`]). Both yield the `Copy`
//! [`PositionedMatch`]; look the pattern and term up on demand with
//! [`CompiledMatcher::pattern`] and [`CompiledMatcher::term`].
//!
//! ```
//! use terraphim_automata::{MatchOverlap, MatcherBuilder, MatcherOptions};
//! use terraphim_types::{NormalizedTerm, NormalizedTermValue};
//!
//! let mut builder = MatcherBuilder::new(MatcherOptions::default());
//! for word in ["rust", "rust async"] {
//!     builder.insert(
//!         word.to_string(),
//!         NormalizedTerm::with_auto_id(NormalizedTermValue::from(word)),
//!     )?;
//! }
//! let matcher = builder.build()?;
//!
//! let text = "rust async and rust";
//! let found: Vec<(&str, usize, usize)> = matcher
//!     .find_positions(text)
//!     .map(|m| (matcher.pattern(m.pattern_index).unwrap(), m.start, m.end))
//!     .collect();
//! assert_eq!(found, [("rust async", 0, 10), ("rust", 15, 19)]);
//! # Ok::<(), terraphim_automata::TerraphimAutomataError>(())
//! ```

use std::ops::Range;
use std::sync::Arc;

use aho_corasick::{AhoCorasick, MatchKind};
use serde::{Deserialize, Serialize};
use terraphim_types::NormalizedTerm;

use crate::matcher::Matched;
use crate::{Result, TerraphimAutomataError};

/// Default minimum pattern length, mirroring `find_matches`.
pub const DEFAULT_MIN_PATTERN_LENGTH: usize = 2;

/// How a [`CompiledMatcher`] treats occurrences that overlap one another.
///
/// Marked `#[non_exhaustive]`: further overlap policies may be added in a
/// minor release, so a `match` on this enum outside this crate needs a
/// wildcard arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum MatchOverlap {
    /// Report at most one match per position, preferring the longest pattern
    /// that starts leftmost; overlapping occurrences are never reported.
    /// This is the behaviour of [`crate::matcher::find_matches`].
    #[default]
    LeftmostLongest,
    /// Report every occurrence of every pattern, including occurrences that
    /// overlap each other or lie inside a longer pattern's occurrence.
    All,
}

/// Explicit construction configuration for a [`CompiledMatcher`].
///
/// This is the only configuration that needs persisting alongside the source
/// patterns; the compiled automaton itself can always be rebuilt from
/// patterns + options.
///
/// The struct is `#[non_exhaustive]`: fields stay public for reading, but
/// code outside this crate cannot build it with a struct literal (not even
/// with `..MatcherOptions::default()`). Start from
/// [`MatcherOptions::default`] and adjust it with the chainable `with_*`
/// setters, so options added later keep their default
/// (backwards-compatible) behaviour.
///
/// # Example
///
/// ```
/// use terraphim_automata::{MatchOverlap, MatcherOptions};
///
/// let options = MatcherOptions::default()
///     .with_overlap(MatchOverlap::All)
///     .with_word_boundaries(false);
///
/// assert_eq!(options.overlap, MatchOverlap::All);
/// assert!(!options.word_boundaries);
/// // Untouched fields keep their defaults.
/// assert!(options.case_insensitive);
/// assert_eq!(
///     options.min_pattern_length,
///     terraphim_automata::compiled::DEFAULT_MIN_PATTERN_LENGTH,
/// );
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct MatcherOptions {
    /// ASCII case-insensitive matching (defaults to `true`, matching the
    /// existing `find_matches` behaviour).
    pub case_insensitive: bool,
    /// Minimum accepted pattern length in bytes. Shorter patterns are
    /// rejected at build time.
    pub min_pattern_length: usize,
    /// Overlap policy (defaults to [`MatchOverlap::LeftmostLongest`]).
    ///
    /// Absent from data persisted before this option existed; such data
    /// deserialises with the default.
    #[serde(default)]
    pub overlap: MatchOverlap,
    /// Only report matches that start and end on a word boundary (defaults
    /// to `true`, the `find_matches` behaviour). `false` reports matches
    /// inside words too, such as `cat` in `concatenate`.
    ///
    /// Absent from data persisted before this option existed; such data
    /// deserialises with the default.
    #[serde(default = "default_word_boundaries")]
    pub word_boundaries: bool,
    /// Accept patterns made only of whitespace, such as `"  "` (defaults to
    /// `false`, which rejects them as blank). Empty patterns are always
    /// rejected, because an empty pattern would match at every offset.
    ///
    /// The [`MatcherOptions::word_boundaries`] filter still applies: with it
    /// on, a whitespace match directly next to a word character (the gap in
    /// `"a  b"`) is dropped, so callers searching for whitespace runs usually
    /// also want `with_word_boundaries(false)`.
    ///
    /// Absent from data persisted before this option existed; such data
    /// deserialises with the default. Always serialised, so non-self-describing
    /// formats such as bincode round-trip.
    #[serde(default)]
    pub allow_blank_patterns: bool,
}

/// Serde default for [`MatcherOptions::word_boundaries`].
fn default_word_boundaries() -> bool {
    true
}

impl Default for MatcherOptions {
    fn default() -> Self {
        Self {
            case_insensitive: true,
            min_pattern_length: DEFAULT_MIN_PATTERN_LENGTH,
            overlap: MatchOverlap::LeftmostLongest,
            word_boundaries: default_word_boundaries(),
            allow_blank_patterns: false,
        }
    }
}

impl MatcherOptions {
    /// Set [`MatcherOptions::case_insensitive`].
    ///
    /// ```
    /// use terraphim_automata::MatcherOptions;
    ///
    /// let options = MatcherOptions::default().with_case_insensitive(false);
    /// assert!(!options.case_insensitive);
    /// ```
    #[must_use]
    pub const fn with_case_insensitive(mut self, case_insensitive: bool) -> Self {
        self.case_insensitive = case_insensitive;
        self
    }

    /// Set [`MatcherOptions::min_pattern_length`] (in bytes).
    ///
    /// ```
    /// use terraphim_automata::MatcherOptions;
    ///
    /// let options = MatcherOptions::default().with_min_pattern_length(1);
    /// assert_eq!(options.min_pattern_length, 1);
    /// ```
    #[must_use]
    pub const fn with_min_pattern_length(mut self, min_pattern_length: usize) -> Self {
        self.min_pattern_length = min_pattern_length;
        self
    }

    /// Set [`MatcherOptions::overlap`].
    ///
    /// ```
    /// use terraphim_automata::{MatchOverlap, MatcherOptions};
    ///
    /// let options = MatcherOptions::default().with_overlap(MatchOverlap::All);
    /// assert_eq!(options.overlap, MatchOverlap::All);
    /// ```
    #[must_use]
    pub const fn with_overlap(mut self, overlap: MatchOverlap) -> Self {
        self.overlap = overlap;
        self
    }

    /// Set [`MatcherOptions::word_boundaries`].
    ///
    /// ```
    /// use terraphim_automata::MatcherOptions;
    ///
    /// let options = MatcherOptions::default().with_word_boundaries(false);
    /// assert!(!options.word_boundaries);
    /// ```
    #[must_use]
    pub const fn with_word_boundaries(mut self, word_boundaries: bool) -> Self {
        self.word_boundaries = word_boundaries;
        self
    }

    /// Set [`MatcherOptions::allow_blank_patterns`].
    ///
    /// ```
    /// use terraphim_automata::{MatcherBuilder, MatcherOptions};
    /// use terraphim_types::{NormalizedTerm, NormalizedTermValue};
    ///
    /// let options = MatcherOptions::default().with_allow_blank_patterns(true);
    /// assert!(options.allow_blank_patterns);
    ///
    /// let mut builder = MatcherBuilder::new(options);
    /// let term = NormalizedTerm::with_auto_id(NormalizedTermValue::from("gap"));
    /// assert!(builder.insert("  ".to_string(), term.clone()).is_ok());
    /// // Empty patterns stay rejected.
    /// assert!(builder.insert(String::new(), term).is_err());
    /// ```
    #[must_use]
    pub const fn with_allow_blank_patterns(mut self, allow_blank_patterns: bool) -> Self {
        self.allow_blank_patterns = allow_blank_patterns;
        self
    }
}

/// Where one match lies in the searched text: byte offsets plus the index of
/// the pattern that matched.
///
/// Produced by [`CompiledMatcher::find_positions`] and
/// [`CompiledMatcher::push_positions`]. It is `Copy` and borrows nothing, so
/// producing it never allocates. `pattern_index` is the pattern's insertion
/// position, as used by [`CompiledMatcher::patterns`],
/// [`CompiledMatcher::pattern`] and [`CompiledMatcher::term`].
///
/// The derived ordering compares `start`, then `end`, then `pattern_index`:
/// the order in which [`MatchOverlap::All`] results are reported.
///
/// Marked `#[non_exhaustive]` so fields can be added in a minor release; read
/// the public fields directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub struct PositionedMatch {
    /// Byte offset of the first byte of the match (on a `char` boundary).
    pub start: usize,
    /// Byte offset one past the last byte of the match (on a `char`
    /// boundary).
    pub end: usize,
    /// Insertion index of the pattern that matched.
    pub pattern_index: usize,
}

impl PositionedMatch {
    /// Build a position, for example as an expected value in a test.
    ///
    /// ```
    /// use terraphim_automata::PositionedMatch;
    ///
    /// let m = PositionedMatch::new(3, 7, 0);
    /// assert_eq!((m.start, m.end, m.pattern_index), (3, 7, 0));
    /// ```
    #[must_use]
    pub const fn new(start: usize, end: usize, pattern_index: usize) -> Self {
        Self {
            start,
            end,
            pattern_index,
        }
    }

    /// The matched byte range, `start..end`.
    #[must_use]
    pub const fn range(&self) -> Range<usize> {
        self.start..self.end
    }
}

impl From<aho_corasick::Match> for PositionedMatch {
    #[inline]
    fn from(mat: aho_corasick::Match) -> Self {
        Self {
            start: mat.start(),
            end: mat.end(),
            pattern_index: mat.pattern().as_usize(),
        }
    }
}

/// Validates patterns and compiles them into an immutable [`CompiledMatcher`].
///
/// The builder itself is cheap; the expensive automaton construction happens
/// exactly once in [`MatcherBuilder::build`].
#[derive(Debug, Clone)]
pub struct MatcherBuilder {
    options: MatcherOptions,
    patterns: Vec<(String, NormalizedTerm)>,
}

impl MatcherBuilder {
    /// Create a builder with explicit construction options.
    pub fn new(options: MatcherOptions) -> Self {
        Self {
            options,
            patterns: Vec::new(),
        }
    }

    /// Validate and add a pattern with its normalized term.
    ///
    /// Returns an error for empty patterns, blank (whitespace-only) patterns
    /// unless [`MatcherOptions::allow_blank_patterns`] is set, patterns
    /// shorter than the configured minimum length, and duplicate patterns.
    pub fn insert(&mut self, pattern: String, term: NormalizedTerm) -> Result<&mut Self> {
        let trimmed = pattern.trim();
        let trimmed_len = pattern.len();
        if trimmed.is_empty() && (pattern.is_empty() || !self.options.allow_blank_patterns) {
            let reason = if self.options.allow_blank_patterns {
                "pattern is empty"
            } else {
                "pattern is blank"
            };
            return Err(TerraphimAutomataError::InvalidPattern {
                pattern: pattern.clone(),
                reason: reason.to_string(),
            });
        }
        if trimmed_len < self.options.min_pattern_length {
            return Err(TerraphimAutomataError::InvalidPattern {
                pattern: pattern.clone(),
                reason: format!(
                    "pattern length {} is below the configured minimum {}",
                    trimmed_len, self.options.min_pattern_length
                ),
            });
        }
        if self
            .patterns
            .iter()
            .any(|(existing, _)| existing == &pattern)
        {
            return Err(TerraphimAutomataError::DuplicatePattern(pattern));
        }
        self.patterns.push((pattern, term));
        Ok(self)
    }

    /// Add a pattern, consuming the builder (chaining-friendly).
    pub fn with_pattern(mut self, pattern: String, term: NormalizedTerm) -> Result<Self> {
        self.insert(pattern, term)?;
        Ok(self)
    }

    /// Number of validated patterns staged so far.
    pub fn len(&self) -> usize {
        self.patterns.len()
    }

    /// Whether no patterns are staged.
    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    /// Compile the staged patterns into an immutable [`CompiledMatcher`].
    pub fn build(self) -> Result<CompiledMatcher> {
        // Overlapping search in aho-corasick requires the `Standard` match
        // kind; the leftmost kinds reject `find_overlapping_iter`.
        let match_kind = match self.options.overlap {
            MatchOverlap::LeftmostLongest => MatchKind::LeftmostLongest,
            MatchOverlap::All => MatchKind::Standard,
        };
        let ac = AhoCorasick::builder()
            .match_kind(match_kind)
            .ascii_case_insensitive(self.options.case_insensitive)
            .build(self.patterns.iter().map(|(p, _)| p.as_str()))?;
        Ok(CompiledMatcher {
            inner: Arc::new(CompiledMatcherInner {
                ac,
                patterns: self.patterns,
                options: self.options,
            }),
        })
    }
}

/// Shared immutable state of a [`CompiledMatcher`].
#[derive(Debug)]
struct CompiledMatcherInner {
    ac: AhoCorasick,
    patterns: Vec<(String, NormalizedTerm)>,
    options: MatcherOptions,
}

/// An immutable compiled matcher: build once, query many times.
///
/// Cloning is cheap (shared `Arc` internals). The matcher is `Send + Sync`
/// through ordinary Rust auto-traits (no unsafe impls), so concurrent
/// read-only use is safe. Result buffers may be supplied by the caller via
/// [`CompiledMatcher::push_matches`] to reuse allocations across queries.
#[derive(Debug, Clone)]
pub struct CompiledMatcher {
    inner: Arc<CompiledMatcherInner>,
}

// Auto-traits (Send/Sync) are derived from the fields; assert them so any
// future non-thread-safe field fails compilation here rather than at call sites.
const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    let _ = assert_send_sync::<CompiledMatcher>;
};

impl CompiledMatcher {
    /// Compile a matcher from a [`terraphim_types::Thesaurus`].
    ///
    /// This is the compiled counterpart of [`crate::matcher::find_matches`]:
    /// the thesaurus is consulted once, and the returned matcher reproduces
    /// identical results for any number of subsequent queries.
    pub fn from_thesaurus(
        thesaurus: &terraphim_types::Thesaurus,
        options: MatcherOptions,
    ) -> Result<Self> {
        let mut builder = MatcherBuilder::new(options);
        for (key, term) in thesaurus.into_iter() {
            builder.insert(key.to_string(), term.clone())?;
        }
        builder.build()
    }

    /// The construction configuration used to compile this matcher.
    pub fn options(&self) -> &MatcherOptions {
        &self.inner.options
    }

    /// The validated source patterns backing this matcher, in insertion order.
    ///
    /// Persist these (with [`MatcherOptions`]) to rebuild the matcher later.
    pub fn patterns(&self) -> &[(String, NormalizedTerm)] {
        &self.inner.patterns
    }

    /// The source pattern at `pattern_index` (insertion order), or `None`
    /// when the index is out of range.
    ///
    /// Pairs with [`PositionedMatch::pattern_index`].
    pub fn pattern(&self, pattern_index: usize) -> Option<&str> {
        self.inner
            .patterns
            .get(pattern_index)
            .map(|(pattern, _)| pattern.as_str())
    }

    /// The normalized term of the pattern at `pattern_index` (insertion
    /// order), or `None` when the index is out of range.
    ///
    /// Pairs with [`PositionedMatch::pattern_index`].
    pub fn term(&self, pattern_index: usize) -> Option<&NormalizedTerm> {
        self.inner.patterns.get(pattern_index).map(|(_, term)| term)
    }

    /// Number of compiled patterns.
    pub fn len(&self) -> usize {
        self.inner.patterns.len()
    }

    /// Whether the matcher compiled zero patterns.
    pub fn is_empty(&self) -> bool {
        self.inner.patterns.is_empty()
    }

    /// Find matches in `text`, allocating a fresh result vector.
    ///
    /// With default options, results are identical to
    /// [`crate::matcher::find_matches`] for the same patterns. With
    /// [`MatchOverlap::All`] every occurrence is reported, sorted by start
    /// byte, then end byte, then pattern insertion order. Positions are byte
    /// offsets and always fall on `char` boundaries.
    pub fn find_matches(&self, text: &str, return_positions: bool) -> Result<Vec<Matched>> {
        let mut out = Vec::new();
        self.push_matches_impl(text, return_positions, &mut out)?;
        Ok(out)
    }

    /// Allocation-aware variant: append matches to a caller-owned buffer so
    /// the buffer's capacity is reused across queries.
    ///
    /// The buffer is not cleared first; call `buffer.clear()` between queries
    /// if a fresh result set is required. Because the buffer is caller-owned,
    /// concurrent queries must each use their own buffer.
    pub fn push_matches(&self, text: &str, buffer: &mut Vec<Matched>) -> Result<()> {
        self.push_matches_impl(text, false, buffer)
    }

    /// Iterate matches in the same order as [`CompiledMatcher::find_matches`].
    ///
    /// In the default mode matches are produced lazily without collecting
    /// them. With [`MatchOverlap::All`] the matches are collected and sorted
    /// first, because the automaton reports overlapping matches in order of
    /// their end position.
    pub fn find_iter<'text>(
        &'text self,
        text: &'text str,
    ) -> impl Iterator<Item = Matched> + use<'text> {
        let options = &self.inner.options;
        let raw = match options.overlap {
            MatchOverlap::LeftmostLongest if options.word_boundaries => RawMatches::Leftmost(
                self.inner
                    .ac
                    .find_iter(text)
                    // Same word-boundary rule as the reference `find_matches`,
                    // so the compiled matcher cannot drift from it (see
                    // matcher.rs).
                    .filter(move |mat| {
                        crate::matcher::is_word_boundary_match(text, mat.start(), mat.end())
                    }),
            ),
            MatchOverlap::LeftmostLongest => {
                RawMatches::LeftmostAnywhere(self.inner.ac.find_iter(text))
            }
            MatchOverlap::All => RawMatches::Collected(self.all_matches(text).into_iter()),
        };
        raw.map(move |mat| self.matched_for(mat, false))
    }

    /// Iterate match positions without allocating.
    ///
    /// Yields the same matches as [`CompiledMatcher::find_matches`] with
    /// `return_positions = true` (same overlap policy, word-boundary filter
    /// and case handling) as `Copy` [`PositionedMatch`] values that borrow
    /// nothing; neither the iterator nor the items allocate.
    ///
    /// Order:
    ///
    /// - [`MatchOverlap::LeftmostLongest`]: identical to `find_matches`
    ///   (ascending, non-overlapping).
    /// - [`MatchOverlap::All`]: the order the automaton reports overlapping
    ///   matches in, which is by *end* offset, not the `(start, end,
    ///   pattern_index)` order of `find_matches`. Sorting needs a buffer; use
    ///   [`CompiledMatcher::push_positions`] for sorted results, or collect
    ///   and sort them yourself.
    pub fn find_positions<'a>(
        &'a self,
        text: &'a str,
    ) -> impl Iterator<Item = PositionedMatch> + use<'a> {
        let ac = &self.inner.ac;
        let word_boundaries = self.inner.options.word_boundaries;
        let raw = match self.inner.options.overlap {
            MatchOverlap::LeftmostLongest => RawPositions::Leftmost(ac.find_iter(text)),
            MatchOverlap::All => RawPositions::Overlapping(ac.find_overlapping_iter(text)),
        };
        raw.filter(move |mat| {
            // Same rule as `find_matches`, so the two cannot drift apart.
            !word_boundaries || crate::matcher::is_word_boundary_match(text, mat.start(), mat.end())
        })
        .map(PositionedMatch::from)
    }

    /// Append match positions to a caller-owned buffer, in exactly the order
    /// of [`CompiledMatcher::find_matches`] for every option combination.
    ///
    /// For [`MatchOverlap::All`] the appended matches are sorted by
    /// `(start, end, pattern_index)`; only the newly appended tail is sorted,
    /// entries already in the buffer are left as they are. The buffer is not
    /// cleared first, so call `buffer.clear()` between queries to reuse its
    /// capacity for a fresh result set. Once the buffer has grown to fit the
    /// largest result set, further queries do not allocate.
    pub fn push_positions(&self, text: &str, buffer: &mut Vec<PositionedMatch>) {
        let first_new = buffer.len();
        buffer.extend(self.find_positions(text));
        if self.inner.options.overlap == MatchOverlap::All {
            // `sort_unstable` is in place and allocation-free; equal keys
            // cannot occur because a pattern matches a given range once.
            buffer[first_new..].sort_unstable();
        }
    }

    fn push_matches_impl(
        &self,
        text: &str,
        return_positions: bool,
        out: &mut Vec<Matched>,
    ) -> Result<()> {
        let options = &self.inner.options;
        match options.overlap {
            // The default path: kept as its own loop so it is unchanged by
            // the opt-in options.
            MatchOverlap::LeftmostLongest if options.word_boundaries => {
                for mat in self.inner.ac.find_iter(text) {
                    // Mirrors the reference `find_matches` boundary rule.
                    if !crate::matcher::is_word_boundary_match(text, mat.start(), mat.end()) {
                        continue;
                    }
                    out.push(self.matched_for(mat, return_positions));
                }
            }
            MatchOverlap::LeftmostLongest => {
                out.extend(
                    self.inner
                        .ac
                        .find_iter(text)
                        .map(|mat| self.matched_for(mat, return_positions)),
                );
            }
            MatchOverlap::All => {
                out.extend(
                    self.all_matches(text)
                        .into_iter()
                        .map(|mat| self.matched_for(mat, return_positions)),
                );
            }
        }
        Ok(())
    }

    /// Every occurrence of every pattern (the automaton was built with
    /// `MatchKind::Standard`), boundary-filtered when configured, sorted by
    /// start, end and pattern index.
    fn all_matches(&self, text: &str) -> Vec<aho_corasick::Match> {
        let word_boundaries = self.inner.options.word_boundaries;
        let mut matches: Vec<aho_corasick::Match> = self
            .inner
            .ac
            .find_overlapping_iter(text)
            .filter(|mat| {
                !word_boundaries
                    || crate::matcher::is_word_boundary_match(text, mat.start(), mat.end())
            })
            .collect();
        // Sort the raw matches rather than `Matched` values: `push_matches`
        // does not record positions, so `Matched` carries no sort key.
        matches.sort_unstable_by_key(|mat| (mat.start(), mat.end(), mat.pattern()));
        matches
    }

    fn matched_for(&self, mat: aho_corasick::Match, return_positions: bool) -> Matched {
        let (term, normalized_term) = &self.inner.patterns[mat.pattern().as_usize()];
        Matched {
            term: term.clone(),
            normalized_term: normalized_term.clone(),
            pos: if return_positions {
                Some((mat.start(), mat.end()))
            } else {
                None
            },
        }
    }
}

/// Concrete iterator behind [`CompiledMatcher::find_positions`]: either
/// automaton iterator, without boxing.
enum RawPositions<'a> {
    Leftmost(aho_corasick::FindIter<'a, 'a>),
    Overlapping(aho_corasick::FindOverlappingIter<'a, 'a>),
}

impl Iterator for RawPositions<'_> {
    type Item = aho_corasick::Match;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Leftmost(iter) => iter.next(),
            Self::Overlapping(iter) => iter.next(),
        }
    }
}

/// Concrete iterator behind [`CompiledMatcher::find_iter`], so the default
/// mode stays lazy and allocation-free while [`MatchOverlap::All`] can hand
/// out a sorted, collected list.
enum RawMatches<L, A> {
    /// Leftmost-longest with the word-boundary filter (the default).
    Leftmost(L),
    /// Leftmost-longest without the word-boundary filter.
    LeftmostAnywhere(A),
    /// Every occurrence, collected and sorted.
    Collected(std::vec::IntoIter<aho_corasick::Match>),
}

impl<L, A> Iterator for RawMatches<L, A>
where
    L: Iterator<Item = aho_corasick::Match>,
    A: Iterator<Item = aho_corasick::Match>,
{
    type Item = aho_corasick::Match;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Leftmost(iter) => iter.next(),
            Self::LeftmostAnywhere(iter) => iter.next(),
            Self::Collected(iter) => iter.next(),
        }
    }
}
