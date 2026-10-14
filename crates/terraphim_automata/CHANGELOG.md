# Changelog

All notable changes to `terraphim_automata` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [2.1.0] - 2026-10-05

Additive release for matcher consumers. A slim `default-features = false` build (#88) was evaluated and deferred: LTO already removes the unused code (a wasm32 probe was 502 KB either way), and gating existing APIs would break current `default-features = false` users, so it is left for a future major release.

### Added

- `MatcherOptions::allow_blank_patterns` (default `false`, unchanged behaviour) with the `#[must_use] const fn with_allow_blank_patterns` setter. When `true`, `MatcherBuilder::insert` accepts whitespace-only patterns such as `"  "`, so a consumer can locate whitespace runs with the compiled matcher (for example overlapping `"  "` in `"    "` with `MatchOverlap::All`). Empty patterns are still rejected (now with the reason "pattern is empty" when blank patterns are allowed), as are patterns below `min_pattern_length` and duplicates. The word-boundary filter still applies to blank patterns, so a gap directly next to a word character is only reported with `with_word_boundaries(false)`. Refs #86.
- Allocation-free match positions on `CompiledMatcher` (Refs #87):
  - `find_positions(&self, text) -> impl Iterator<Item = PositionedMatch>`: borrowing iterator that allocates nothing; same matches as `find_matches(text, true)` for every option combination. In `LeftmostLongest` mode the order is identical; in `All` mode items come in automaton order (by end offset), because sorting needs a buffer.
  - `push_positions(&self, text, &mut Vec<PositionedMatch>)`: appends in exactly the `find_matches` order (the appended tail is sorted by start, end and pattern index in `All` mode); no allocation once the buffer has capacity.
  - `pattern(&self, index) -> Option<&str>` and `term(&self, index) -> Option<&NormalizedTerm>` to resolve a match's pattern on demand.
  - `PositionedMatch` (`Copy`, `#[non_exhaustive]`, re-exported at the crate root) with public `start`, `end`, `pattern_index` fields, a `const fn new` constructor (for expected values in consumer tests), a `range()` helper, and an ordering by `(start, end, pattern_index)`.
  - `compiled_matcher_bench` gains `positions_leftmost_longest` and `positions_all` groups comparing `find_matches(text, true)`, `find_positions` and `push_positions`.

### Changed

- Persisted matchers: `allow_blank_patterns: true` also selects format version 2. The field is `#[serde(default)]` everywhere, so version 1 and version 2 envelopes written by 2.0.0 (which lack the field) still load. Like `overlap` and `word_boundaries` in 2.0.0, it is always written (as `false` in default version 1 envelopes, which older readers ignore), so `MatcherOptions` also round-trips through non-self-describing formats such as bincode. A 2.0.0 reader given a 2.1.0 version 2 envelope with whitespace-only patterns ignores the unknown field and then fails the rebuild with `InvalidPattern`; it never silently drops the patterns. Refs #86.

## [2.0.0] - 2026-10-05

Major release (approved 2026-10-04): new matcher options and `#[non_exhaustive]` public types so future additions are not breaking. Migration: build `MatcherOptions` with `MatcherOptions::default().with_...()` setters instead of struct literals, add a wildcard arm when matching `MatchOverlap` or `PersistedMatcherPayload`, and prefer the payload accessors over matching variants.

### Added

- `MatcherOptions::overlap` (new `MatchOverlap` enum, re-exported at the crate root): `LeftmostLongest` (default, unchanged behaviour) or `All`, which makes `CompiledMatcher::find_matches`, `push_matches` and `find_iter` report every occurrence of every pattern, including overlapping ones and short patterns inside longer patterns' occurrences. `All` results are sorted by start byte, then end byte, then pattern insertion order.
- `MatcherOptions::word_boundaries` (default `true`, unchanged behaviour). `false` skips the whole-word filter so matches inside words are reported.
- Both options are `#[serde(default)]` in format version 1: persisted matcher envelopes written before they existed still load, with the previous behaviour.
- Persisted matcher format version 2 (`PersistedMatcherV2`, `PersistedMatcherPayload::V2`) for matchers with non-default overlap or word-boundary options, plus `PersistedMatcherPayload::options`, `patterns` and `version` accessors.
- `compiled_matcher_bench` Criterion benchmark for the default `CompiledMatcher` path.
- Chainable `#[must_use]` `const fn` setters on `MatcherOptions`: `with_case_insensitive`, `with_min_pattern_length`, `with_overlap` and `with_word_boundaries`, for example `MatcherOptions::default().with_overlap(MatchOverlap::All).with_word_boundaries(false)`.

### Changed

- `alternatives_in` and `alternatives_at` now document that they expect default-mode (non-overlapping) matches.
- Persisted matchers: the writer emits format version 2 when `overlap` is not `LeftmostLongest` or `word_boundaries` is `false`, and keeps version 1 for default options, which remain fully readable by older releases. Older readers therefore reject a non-default matcher as an unsupported version instead of silently rebuilding a default-mode one (a behavioural downgrade). The reader accepts versions 1 and 2: version 1 treats missing `overlap` / `word_boundaries` as defaults and honours them when present (pre-release builds wrote non-default options as version 1); version 2 requires every option field. Other versions fail with `UnsupportedVersion`, now decided from the version tag itself, so an invalid option value inside the payload is reported as `MalformedPersisted`. `MATCHER_FORMAT_VERSION` is now `2` (the newest version written).
- **Breaking:** `PersistedMatcherPayload` gains the `V2` variant and is now `#[non_exhaustive]`, so matches outside the crate need a wildcard arm and irrefutable `let PersistedMatcherPayload::V1(..)` patterns no longer compile; use the `options`, `patterns` and `version` accessors. Later format versions can then be added without another major release.
- **Breaking:** `MatcherOptions` and `MatchOverlap` are now `#[non_exhaustive]`, so later options and overlap policies can be added without another major release. Fields stay public for reading, but code outside the crate can no longer build `MatcherOptions` with a struct literal (with or without `..MatcherOptions::default()`) or destructure it exhaustively, and a `match` on `MatchOverlap` needs a wildcard arm. Migrate `MatcherOptions { overlap: MatchOverlap::All, ..MatcherOptions::default() }` to `MatcherOptions::default().with_overlap(MatchOverlap::All)`, chaining the other `with_*` setters as needed.

## [1.22.0] - 2026-10-02

### Added

- `concept_index` module: a reverse index from concept id to its synonyms. `ConceptIndex::from_thesaurus` builds it from a `&Thesaurus`; `synonyms_of`, `concept_name`, and `display_name` read it back. The module also exposes `alternatives_in(matches, offset, index)` and `alternatives_at(text, offset, matcher, index)`, which return the alternatives for the concept matched at a byte offset, plus the `Alternatives` result type (non-exhaustive) carrying the matched `range`, `concept_id`, `matched` key, and `alternatives` replacement text (the concept name in display form, synonyms normalised). Offsets are in bytes; LSP and browser consumers convert from and to UTF-16. This is additive: existing callers see no change, so the semantic-version bump is minor. Refs #75.

## [1.21.2] - 2026-09-10

### Changed

- Replaced the `cached` proc-macro with `memoize` to drop the transitive `web-time` / `wasm-bindgen` dependency chain that surfaced on the 1.21.1 publish. Public Rust API is unchanged; cache semantics (`HashMap<(args), Th>`, lifetime-of-program, no eviction) are equivalent between `#[cached]` and `#[memoize]`.

## [1.21.0] - 2026-08-28

### Changed

- **Breaking:** `find_matches`, `replace_matches`, `extract_paragraphs_from_automata`, and `evaluate` now take `&Thesaurus` instead of an owned `Thesaurus`. Callers must pass a reference (`&thesaurus`). This avoids cloning the thesaurus (and rebuilding Aho-Corasick from owned keys) on every call.

## [1.20.4] - yanked

Yanked on the Terraphim Gitea cargo registry.

## [1.0.0] - 2025-01-22

### Added

#### Core Functionality
- **Autocomplete Index**: FST-based prefix search with O(log n) complexity
- **Fuzzy Search**: Jaro-Winkler and Levenshtein distance algorithms
- **Text Matching**: Aho-Corasick multi-pattern matching
- **Link Generation**: Convert matched terms to Markdown, HTML, or Wiki links
- **Paragraph Extraction**: Extract text context around matched terms

#### API Functions
- `build_autocomplete_index()` - Build FST index from thesaurus
- `autocomplete_search()` - Exact prefix matching
- `fuzzy_autocomplete_search()` - Fuzzy matching with Jaro-Winkler
- `fuzzy_autocomplete_search_levenshtein()` - Fuzzy matching with Levenshtein distance
- `find_matches()` - Multi-pattern text matching
- `replace_matches()` - Replace matches with links (Markdown/HTML/Wiki)
- `extract_paragraphs_from_automata()` - Context extraction around matches
- `serialize_autocomplete_index()` / `deserialize_autocomplete_index()` - Index persistence

#### Thesaurus Loading
- `load_thesaurus()` - Async loading from file or HTTP URL
- `load_thesaurus_from_json()` - Sync JSON parsing
- `load_thesaurus_from_json_and_replace()` - Combined load + replace operation
- `AutomataPath` enum for local/remote file handling

#### Types
- `AutocompleteIndex` - FST-based index with metadata
- `AutocompleteResult` - Search result with score
- `AutocompleteMetadata` - Term metadata (ID, URL, usage count)
- `AutocompleteConfig` - Index configuration
- `Matched` - Text match with position and metadata
- `LinkType` - Link format enum (MarkdownLinks, HTMLLinks, WikiLinks)
- `TerraphimAutomataError` - Comprehensive error types

#### Builders
- `ThesaurusBuilder` trait - Custom thesaurus parsers
- `Logseq` builder - Parse Logseq markdown files

### Features
- `remote-loading`: Enable async HTTP loading (requires tokio + reqwest)
- `tokio-runtime`: Tokio async runtime support
- `typescript`: TypeScript type generation via tsify
- `wasm`: WebAssembly compilation support

### Performance
- Sub-2ms autocomplete for 10,000+ terms
- O(n+m) text matching complexity
- ~100KB memory per 1,000 terms in FST
- Streaming text replacement for large documents

### Documentation
- Comprehensive module-level documentation with examples
- Rustdoc comments on all public functions and types
- Usage examples for:
  - Autocomplete with fuzzy matching
  - Text matching and link generation
  - Thesaurus loading (local and remote)
  - WASM browser integration
- README with quick start guide
- WASM package project in `wasm/`

### WASM Support
- Full browser compatibility
- TypeScript type definitions
- Example integration at `wasm/`
- Compatible with Chrome 57+, Firefox 52+, Safari 11+
- ~200KB compressed bundle size (release build)

### Implementation Details
- Aho-Corasick automata for fast multi-pattern matching
- FST (finite state transducer) for memory-efficient prefix search
- Cached fuzzy matching with `cached` crate
- Case-insensitive matching support
- Position tracking for context extraction
- Streaming replacement for memory efficiency

[Unreleased]: https://github.com/terraphim/terraphim-ai/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/terraphim/terraphim-ai/releases/tag/v1.0.0
