use aho_corasick::{AhoCorasick, MatchKind};
use terraphim_types::{NormalizedTerm, NormalizedTermValue, RoleName, Thesaurus};

use crate::url_protector::UrlProtector;

use crate::{Result, TerraphimAutomataError};

#[derive(Debug, PartialEq, Clone)]
pub struct Matched {
    pub term: String,
    pub normalized_term: NormalizedTerm,
    pub pos: Option<(usize, usize)>,
}

/// Minimum pattern length for find_matches to prevent spurious matches.
const MIN_FIND_PATTERN_LENGTH: usize = 2;

/// Characters that continue a word for the purposes of match-boundary checks.
///
/// Deliberately restricted to *ASCII* alphanumerics (plus `_`). Scripts that do
/// not delimit words with whitespace (CJK, Thai, ...) are made up of non-ASCII
/// alphanumerics, so they are treated as boundaries and continue to match
/// mid-"word" as they must.
fn is_word_continuation(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// True when the byte range `start..end` of `text` is delimited by word
/// boundaries on both sides.
///
/// Without this check Aho-Corasick reports matches *inside* longer words: a
/// legitimate two-letter thesaurus term such as `ce` ("capability evaluation")
/// otherwise matches the middle of `concept`, poisoning `concepts_matched` and
/// suppressing the wildcard fallback that depends on it. Filtering by pattern
/// length alone (see [`MIN_FIND_PATTERN_LENGTH`]) cannot fix this, because the
/// term really is a valid term -- it is the *position* that is wrong.
///
/// `start` and `end` come from Aho-Corasick matches over valid UTF-8 patterns,
/// so they are always char boundaries and the slicing below cannot panic.
pub(crate) fn is_word_boundary_match(text: &str, start: usize, end: usize) -> bool {
    let before_ok = text[..start]
        .chars()
        .next_back()
        .is_none_or(|c| !is_word_continuation(c));
    let after_ok = text[end..]
        .chars()
        .next()
        .is_none_or(|c| !is_word_continuation(c));
    before_ok && after_ok
}

/// Finds thesaurus matches within `text`, optionally recording byte positions.
///
/// Borrows the thesaurus so callers can reuse it without cloning. Patterns are
/// fed to Aho-Corasick as `&str`; only match records allocate.
pub fn find_matches(
    text: &str,
    thesaurus: &Thesaurus,
    return_positions: bool,
) -> Result<Vec<Matched>> {
    // Filter out empty and too-short patterns without cloning the thesaurus.
    let valid: Vec<(&NormalizedTermValue, &NormalizedTerm)> = thesaurus
        .into_iter()
        .filter(|(key, _value)| {
            let pattern_str = key.as_str();
            if pattern_str.trim().is_empty() || pattern_str.len() < MIN_FIND_PATTERN_LENGTH {
                log::warn!(
                    "Skipping invalid pattern in find_matches: {:?} (length {} < {})",
                    pattern_str,
                    pattern_str.len(),
                    MIN_FIND_PATTERN_LENGTH
                );
                false
            } else {
                true
            }
        })
        .collect();

    if valid.is_empty() {
        log::debug!("No valid patterns for find_matches, returning empty");
        return Ok(Vec::new());
    }

    log::debug!(
        "Building find_matches automaton with {} valid patterns",
        valid.len()
    );

    let ac = AhoCorasick::builder()
        .match_kind(MatchKind::LeftmostLongest)
        .ascii_case_insensitive(true)
        .build(valid.iter().map(|(key, _)| key.as_str()))?;

    let mut matches: Vec<Matched> = Vec::new();
    for mat in ac.find_iter(text) {
        // Reject matches that land inside a longer word (e.g. `ce` in `concept`).
        if !is_word_boundary_match(text, mat.start(), mat.end()) {
            continue;
        }

        let (term, normalized_term) = valid[mat.pattern().as_usize()];

        matches.push(Matched {
            term: term.to_string(),
            normalized_term: (*normalized_term).clone(),
            pos: if return_positions {
                Some((mat.start(), mat.end()))
            } else {
                None
            },
        });
    }
    Ok(matches)
}

/// Compute the list of thesaurus concepts matched by `query`.
///
/// Wraps [`find_matches`] with the small ergonomic shim used by callers
/// that already hold a [`Thesaurus`] and only care about the matched terms,
/// not the full [`Matched`] records or positions. Returns the term values
/// of every matched entry.
///
/// On any matching error this returns an empty `Vec` and logs at `debug!`,
/// because all current callers (robot-mode search envelope) treat
/// "concepts_matched" as a best-effort enrichment field that must never
/// fail the surrounding response.
///
/// # Example
///
/// ```rust,ignore
/// use terraphim_automata::compute_concepts_matched;
/// use terraphim_types::Thesaurus;
///
/// let thesaurus = Thesaurus::new("role-eng".into());
/// // ... populate thesaurus ...
/// let concepts = compute_concepts_matched("learning rust async", &thesaurus);
/// ```
pub fn compute_concepts_matched(query: &str, thesaurus: &Thesaurus) -> Vec<String> {
    match find_matches(query, thesaurus, false) {
        Ok(matches) => matches.into_iter().map(|m| m.term).collect(),
        Err(e) => {
            log::debug!("compute_concepts_matched: find_matches failed: {}", e);
            Vec::new()
        }
    }
}

/// Build a minimal [`Thesaurus`] from a slice of plain term strings, assigning
/// each an auto-generated unique id via [`NormalizedTerm::with_auto_id`].
///
/// Intended for callers that receive lossy thesaurus data over the wire
/// (e.g. an HTTP API that returns only `HashMap<String, String>` values)
/// and need a `Thesaurus` for [`find_matches`] / [`compute_concepts_matched`]
/// without committing to a fake id (`1u64` for every term).
///
/// The first argument is used to label the thesaurus for diagnostics; pass
/// the role name or any meaningful tag.
///
/// # Example
///
/// ```rust,ignore
/// use terraphim_automata::thesaurus_from_terms;
/// use terraphim_types::RoleName;
///
/// let role = RoleName::new("Engineer");
/// let values = vec!["rust".to_string(), "async".to_string()];
/// let thesaurus = thesaurus_from_terms(&role, values.iter().map(String::as_str));
/// ```
pub fn thesaurus_from_terms<'a, I>(role: &RoleName, values: I) -> Thesaurus
where
    I: IntoIterator<Item = &'a str>,
{
    let mut thesaurus = Thesaurus::new(format!("role-{}", role));
    for value in values {
        let nv = NormalizedTermValue::from(value);
        let term = NormalizedTerm::with_auto_id(nv.clone());
        thesaurus.insert(nv, term);
    }
    thesaurus
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LinkType {
    WikiLinks,
    HTMLLinks,
    MarkdownLinks,
    #[default]
    PlainText,
}

/// Minimum pattern length to prevent spurious matches.
/// Patterns shorter than this are filtered out to avoid:
/// - Empty patterns matching at every character position
/// - Single-character patterns causing excessive matches
const MIN_PATTERN_LENGTH: usize = 2;

/// Replace matches in text using the thesaurus.
///
/// Uses `display()` method on `NormalizedTerm` to get the case-preserved
/// display value for replacement output.
///
/// URLs (http, https, mailto, email addresses) are protected from replacement
/// to prevent corruption of links.
///
/// Patterns shorter than MIN_PATTERN_LENGTH (2) are filtered out to prevent
/// spurious matches at every character position.
pub fn replace_matches(text: &str, thesaurus: &Thesaurus, link_type: LinkType) -> Result<Vec<u8>> {
    // Protect URLs from replacement
    let protector = UrlProtector::new();
    let (masked_text, protected_urls) = protector.mask_urls(text);

    let mut patterns: Vec<String> = Vec::new();
    let mut replace_with: Vec<String> = Vec::new();

    for (key, value) in thesaurus {
        let pattern_str = key.as_str().to_string();

        // Skip empty or too-short patterns to prevent spurious matches
        // Empty patterns match at every character position, causing text like
        // "exmatching_and_iterators_in_rustpmatching..." to appear
        if pattern_str.trim().is_empty() || pattern_str.len() < MIN_PATTERN_LENGTH {
            log::warn!(
                "Skipping invalid pattern: {:?} (length {} < {})",
                pattern_str,
                pattern_str.len(),
                MIN_PATTERN_LENGTH
            );
            continue;
        }

        // Use display() to get case-preserved value for output
        let display_text = value.display();
        let replacement = match link_type {
            LinkType::WikiLinks => format!("[[{}]]", display_text),
            LinkType::HTMLLinks => render_html_link_or_text(display_text, value.url.as_deref()),
            LinkType::MarkdownLinks => {
                render_markdown_link_or_text(display_text, value.url.as_deref())
            }
            LinkType::PlainText => display_text.to_string(),
        };

        patterns.push(pattern_str);
        replace_with.push(replacement);
    }

    // Validate alignment - patterns and replacements must be 1:1
    debug_assert_eq!(
        patterns.len(),
        replace_with.len(),
        "Pattern/replacement vector mismatch: {} patterns vs {} replacements",
        patterns.len(),
        replace_with.len()
    );

    // If no valid patterns, return original text unchanged
    if patterns.is_empty() {
        log::debug!("No valid patterns to replace, returning original text");
        return Ok(text.as_bytes().to_vec());
    }

    log::debug!("Building automaton with {} valid patterns", patterns.len());

    let ac = AhoCorasick::builder()
        .match_kind(MatchKind::LeftmostLongest)
        .ascii_case_insensitive(true)
        .build(&patterns)?;

    // Perform replacement on masked text
    let replaced = ac.replace_all(&masked_text, &replace_with);

    // Restore protected URLs
    let result = protector.restore_urls(&replaced, &protected_urls);

    Ok(result.into_bytes())
}

fn render_html_link_or_text(label: &str, url: Option<&str>) -> String {
    let escaped_label = escape_html_text(label);
    let Some(url) = url.filter(|url| is_safe_link_url(url)) else {
        return escaped_label;
    };

    format!(
        "<a href=\"{}\">{}</a>",
        escape_html_attr(url),
        escaped_label
    )
}

fn render_markdown_link_or_text(label: &str, url: Option<&str>) -> String {
    let escaped_label = escape_markdown_link_label(label);
    let Some(url) = url.filter(|url| is_safe_link_url(url)) else {
        return escaped_label;
    };

    format!(
        "[{}]({})",
        escaped_label,
        escape_markdown_link_destination(url)
    )
}

fn is_safe_link_url(url: &str) -> bool {
    let trimmed = url.trim();
    if trimmed.is_empty() || contains_control_or_format_char(trimmed) {
        return false;
    }

    let scheme_candidate = trimmed.split(['/', '?', '#']).next().unwrap_or(trimmed);
    if let Some(colon_index) = scheme_candidate.find(':') {
        let scheme = &scheme_candidate[..colon_index];
        if scheme.is_empty()
            || !scheme
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
        {
            return false;
        }

        matches!(
            scheme.to_ascii_lowercase().as_str(),
            "http" | "https" | "mailto"
        )
    } else {
        !trimmed.starts_with("//")
    }
}

fn contains_control_or_format_char(value: &str) -> bool {
    value.chars().any(|ch| {
        ch.is_control()
            || matches!(
                ch,
                '\u{061c}'
                    | '\u{200e}'
                    | '\u{200f}'
                    | '\u{202a}'..='\u{202e}'
                    | '\u{2066}'..='\u{2069}'
            )
    })
}

fn escape_html_text(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_control() || matches!(ch, '\t' | '\n' | '\r'))
        .flat_map(|ch| match ch {
            '&' => "&amp;".chars().collect::<Vec<_>>(),
            '<' => "&lt;".chars().collect(),
            '>' => "&gt;".chars().collect(),
            '"' => "&quot;".chars().collect(),
            '\'' => "&#39;".chars().collect(),
            _ => vec![ch],
        })
        .collect()
}

fn escape_html_attr(value: &str) -> String {
    escape_html_text(value)
}

fn escape_markdown_link_label(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        if matches!(ch, '\\' | '[' | ']' | '(' | ')') {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped
}

fn escape_markdown_link_destination(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            ' ' => escaped.push_str("%20"),
            '\\' | '(' | ')' => {
                escaped.push('\\');
                escaped.push(ch);
            }
            _ => escaped.push(ch),
        }
    }
    escaped
}

// tests

/// Extract the paragraph text starting at each automata term match.
///
/// For every matched term in `text`, returns the substring from the start of the term
/// until the end of the containing paragraph (first blank line or end-of-text).
pub fn extract_paragraphs_from_automata(
    text: &str,
    thesaurus: &Thesaurus,
    include_term: bool,
) -> Result<Vec<(Matched, String)>> {
    let matches = find_matches(text, thesaurus, true)?;
    let mut results: Vec<(Matched, String)> = Vec::new();

    for m in matches.into_iter() {
        let (start, end) = m.pos.ok_or_else(|| {
            TerraphimAutomataError::Dict("Positions were not returned".to_string())
        })?;

        // Start at term start (or right after the term) depending on flag
        let paragraph_start = if include_term { start } else { end };
        let paragraph_end = find_paragraph_end(text, end);

        if paragraph_start <= paragraph_end && paragraph_start < text.len() {
            let slice = &text[paragraph_start..paragraph_end];
            results.push((m, slice.to_string()));
        }
    }

    Ok(results)
}

/// Find the end of the paragraph starting after `from_index`.
/// Paragraphs are separated by a blank line, matched by the earliest of
/// "\r\n\r\n", "\n\n", or "\r\r". If none found, returns end of text.
fn find_paragraph_end(text: &str, from_index: usize) -> usize {
    if from_index >= text.len() {
        return text.len();
    }
    let tail = &text[from_index..];
    let mut end_rel: Option<usize> = None;
    for sep in ["\r\n\r\n", "\n\n", "\r\r"] {
        if let Some(i) = tail.find(sep) {
            end_rel = Some(match end_rel {
                Some(cur) => cur.min(i),
                None => i,
            });
        }
    }
    match end_rel {
        Some(i) => from_index + i,
        None => text.len(),
    }
}

#[cfg(test)]
mod compute_concepts_matched_tests {
    use super::*;
    use terraphim_types::RoleName;

    #[test]
    fn empty_thesaurus_returns_empty() {
        let thesaurus = Thesaurus::new("empty".to_string());
        assert!(compute_concepts_matched("rust async tokio", &thesaurus).is_empty());
    }

    #[test]
    fn single_term_match() {
        let role = RoleName::new("Engineer");
        let thesaurus = thesaurus_from_terms(&role, ["rust"]);
        let matches = compute_concepts_matched("learning rust today", &thesaurus);
        assert_eq!(matches, vec!["rust".to_string()]);
    }

    #[test]
    fn case_insensitive() {
        let role = RoleName::new("Engineer");
        let thesaurus = thesaurus_from_terms(&role, ["rust"]);
        let matches = compute_concepts_matched("RUST is great", &thesaurus);
        // The matched term carries the case from the input text.
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].to_lowercase(), "rust");
    }

    #[test]
    fn parity_full_vs_string_built_thesaurus() {
        // Regression test: a Thesaurus built from full NormalizedTerm records
        // (offline robot-search path) and one built via thesaurus_from_terms
        // (server robot-search path, lossy HTTP API) must produce identical
        // compute_concepts_matched output for the same query.
        let role = RoleName::new("Engineer");
        let terms = ["rust", "tokio", "async"];

        // Offline shape: ids carried through from the in-memory thesaurus.
        let mut offline = Thesaurus::new("offline".to_string());
        for (i, t) in terms.iter().enumerate() {
            let nv = NormalizedTermValue::from(*t);
            offline.insert(nv.clone(), NormalizedTerm::new(i as u64 + 100, nv));
        }

        // Server shape: only string values available; ids assigned by helper.
        let server = thesaurus_from_terms(&role, terms.iter().copied());

        let query = "Today I am learning rust async with tokio";
        let mut a = compute_concepts_matched(query, &offline);
        let mut b = compute_concepts_matched(query, &server);
        a.sort();
        b.sort();
        assert_eq!(a, b, "offline/server thesaurus shapes must agree");
    }

    #[test]
    fn multiple_terms_unique_ids() {
        let role = RoleName::new("Engineer");
        let thesaurus = thesaurus_from_terms(&role, ["rust", "tokio", "async"]);
        // with_auto_id must give distinct ids; verify by collecting the
        // underlying NormalizedTerm.id values from the thesaurus.
        let ids: std::collections::HashSet<u64> =
            thesaurus.into_iter().map(|(_, term)| term.id).collect();
        assert_eq!(ids.len(), 3, "with_auto_id should produce unique ids");
    }
}

#[cfg(test)]
mod paragraph_tests {
    use super::*;
    use terraphim_types::{NormalizedTerm, NormalizedTermValue, Thesaurus};

    #[test]
    fn extracts_paragraph_from_term() {
        let mut thesaurus = Thesaurus::new("test".to_string());
        let norm = NormalizedTerm::new(1, NormalizedTermValue::from("lorem"));
        thesaurus.insert(NormalizedTermValue::from("lorem"), norm);

        let text = "Intro\n\nlorem ipsum dolor sit amet,\nconsectetur adipiscing elit.\n\nNext paragraph starts here.";

        let results = extract_paragraphs_from_automata(text, &thesaurus, true).unwrap();
        assert_eq!(results.len(), 1);
        let (_m, para) = &results[0];
        assert!(para.starts_with("lorem ipsum"));
        assert!(para.contains("consectetur"));
        assert!(!para.contains("Next paragraph"));
    }
}

#[cfg(test)]
mod replacement_bug_tests {
    use super::*;
    use terraphim_types::{NormalizedTerm, NormalizedTermValue, Thesaurus};

    /// Regression test for the bug where empty patterns caused text to be
    /// inserted between every character.
    ///
    /// Bug manifestation: "npm install express" became
    /// "bun install exmatching_and_iterators_in_rustpmatching..."
    #[test]
    fn html_links_escape_labels_and_attributes() {
        let mut thesaurus = Thesaurus::new("security".to_string());
        let term = NormalizedTerm::new(1, NormalizedTermValue::from("dangerous"))
            .with_display_value("<b>Dangerous & \"quoted\"</b>".to_string())
            .with_url("https://example.com/?q=\"x\"&next=<tag>".to_string());
        thesaurus.insert(NormalizedTermValue::from("dangerous"), term);

        let result = replace_matches("dangerous", &thesaurus, LinkType::HTMLLinks).unwrap();
        let result_str = String::from_utf8(result).unwrap();

        assert_eq!(
            result_str,
            "<a href=\"https://example.com/?q=&quot;x&quot;&amp;next=&lt;tag&gt;\">&lt;b&gt;Dangerous &amp; &quot;quoted&quot;&lt;/b&gt;</a>"
        );
    }

    #[test]
    fn markdown_links_escape_labels_and_destinations() {
        let mut thesaurus = Thesaurus::new("security".to_string());
        let term = NormalizedTerm::new(1, NormalizedTermValue::from("dangerous"))
            .with_display_value(r#"Use [safe](text) \ now"#.to_string())
            .with_url("https://example.com/a path_(draft)".to_string());
        thesaurus.insert(NormalizedTermValue::from("dangerous"), term);

        let result = replace_matches("dangerous", &thesaurus, LinkType::MarkdownLinks).unwrap();
        let result_str = String::from_utf8(result).unwrap();

        assert_eq!(
            result_str,
            r#"[Use \[safe\]\(text\) \\ now](https://example.com/a%20path_\(draft\))"#
        );
    }

    #[test]
    fn unsafe_link_schemes_render_inert_visible_text() {
        for url in [
            "javascript:alert(1)",
            "JaVaScRiPt:alert(1)",
            "java%73cript:alert(1)",
            "data:text/html,<script>",
            "https://example.com/has\nnewline",
        ] {
            let mut thesaurus = Thesaurus::new("security".to_string());
            let term = NormalizedTerm::new(1, NormalizedTermValue::from("dangerous"))
                .with_display_value("Dangerous".to_string())
                .with_url(url.to_string());
            thesaurus.insert(NormalizedTermValue::from("dangerous"), term);

            let html = String::from_utf8(
                replace_matches("dangerous", &thesaurus, LinkType::HTMLLinks).unwrap(),
            )
            .unwrap();
            let markdown = String::from_utf8(
                replace_matches("dangerous", &thesaurus, LinkType::MarkdownLinks).unwrap(),
            )
            .unwrap();

            assert_eq!(html, "Dangerous");
            assert_eq!(markdown, "Dangerous");
            assert!(!html.contains("href"));
            assert!(!markdown.contains(url));
        }
    }

    #[test]
    fn safe_link_schemes_are_rendered() {
        for url in [
            "https://example.com/path",
            "http://example.com/path",
            "mailto:team@example.com",
            "/relative/path",
            "../relative/path",
            "#fragment",
        ] {
            let mut thesaurus = Thesaurus::new("security".to_string());
            let term = NormalizedTerm::new(1, NormalizedTermValue::from("safe"))
                .with_display_value("Safe".to_string())
                .with_url(url.to_string());
            thesaurus.insert(NormalizedTermValue::from("safe"), term);

            let html = String::from_utf8(
                replace_matches("safe", &thesaurus, LinkType::HTMLLinks).unwrap(),
            )
            .unwrap();
            assert!(html.contains("href="), "{url} should be rendered as a link");
        }
    }

    #[test]
    fn test_empty_pattern_does_not_cause_spurious_insertions() {
        let mut thesaurus = Thesaurus::new("test".to_string());

        // Simulate the bug: empty pattern with a display value
        let bad_nterm = NormalizedTerm::new(
            1,
            NormalizedTermValue::from("matching_and_iterators_in_rust"),
        )
        .with_display_value("matching_and_iterators_in_rust".to_string());
        thesaurus.insert(NormalizedTermValue::from(""), bad_nterm);

        // Add a valid pattern
        let bun_nterm = NormalizedTerm::new(2, NormalizedTermValue::from("bun"))
            .with_display_value("bun".to_string());
        thesaurus.insert(NormalizedTermValue::from("npm"), bun_nterm);

        let result =
            replace_matches("npm install express", &thesaurus, LinkType::PlainText).unwrap();
        let result_str = String::from_utf8(result).unwrap();

        // Should NOT have spurious insertions between characters
        assert_eq!(result_str, "bun install express");
        assert!(!result_str.contains("matching_and_iterators_in_rust"));
    }

    #[test]
    fn test_single_char_pattern_is_filtered() {
        let mut thesaurus = Thesaurus::new("test".to_string());

        // Single character pattern - should be filtered
        let single_char_nterm = NormalizedTerm::new(1, NormalizedTermValue::from("expanded"))
            .with_display_value("expanded".to_string());
        thesaurus.insert(NormalizedTermValue::from("e"), single_char_nterm);

        // Valid pattern
        let bun_nterm = NormalizedTerm::new(2, NormalizedTermValue::from("bun"))
            .with_display_value("bun".to_string());
        thesaurus.insert(NormalizedTermValue::from("npm"), bun_nterm);

        let result =
            replace_matches("npm install express", &thesaurus, LinkType::PlainText).unwrap();
        let result_str = String::from_utf8(result).unwrap();

        // Single-char pattern should be filtered, only npm->bun replacement should happen
        assert_eq!(result_str, "bun install express");
        assert!(!result_str.contains("expanded"));
    }

    #[test]
    fn test_whitespace_only_pattern_is_filtered() {
        let mut thesaurus = Thesaurus::new("test".to_string());

        // Whitespace-only pattern - should be filtered
        let ws_nterm = NormalizedTerm::new(1, NormalizedTermValue::from("space"))
            .with_display_value("space".to_string());
        thesaurus.insert(NormalizedTermValue::from("   "), ws_nterm);

        // Valid pattern
        let bun_nterm = NormalizedTerm::new(2, NormalizedTermValue::from("bun"))
            .with_display_value("bun".to_string());
        thesaurus.insert(NormalizedTermValue::from("npm"), bun_nterm);

        let result =
            replace_matches("npm install express", &thesaurus, LinkType::PlainText).unwrap();
        let result_str = String::from_utf8(result).unwrap();

        assert_eq!(result_str, "bun install express");
        assert!(!result_str.contains("space"));
    }

    #[test]
    fn test_valid_replacement_still_works() {
        let mut thesaurus = Thesaurus::new("test".to_string());

        // Valid patterns
        let bun_nterm = NormalizedTerm::new(1, NormalizedTermValue::from("bun"))
            .with_display_value("bun".to_string());
        thesaurus.insert(NormalizedTermValue::from("npm"), bun_nterm);

        let yarn_nterm = NormalizedTerm::new(2, NormalizedTermValue::from("bun"))
            .with_display_value("bun".to_string());
        thesaurus.insert(NormalizedTermValue::from("yarn"), yarn_nterm);

        let result = replace_matches(
            "npm install && yarn add lodash",
            &thesaurus,
            LinkType::PlainText,
        )
        .unwrap();
        let result_str = String::from_utf8(result).unwrap();

        assert_eq!(result_str, "bun install && bun add lodash");
    }

    #[test]
    fn test_empty_thesaurus_returns_original() {
        let thesaurus = Thesaurus::new("test".to_string());

        let result =
            replace_matches("npm install express", &thesaurus, LinkType::PlainText).unwrap();
        let result_str = String::from_utf8(result).unwrap();

        assert_eq!(result_str, "npm install express");
    }

    #[test]
    fn test_find_matches_filters_empty_patterns() {
        let mut thesaurus = Thesaurus::new("test".to_string());

        // Empty pattern
        let empty_nterm = NormalizedTerm::new(1, NormalizedTermValue::from("empty"))
            .with_display_value("empty".to_string());
        thesaurus.insert(NormalizedTermValue::from(""), empty_nterm);

        // Valid pattern
        let test_nterm = NormalizedTerm::new(2, NormalizedTermValue::from("test"))
            .with_display_value("test".to_string());
        thesaurus.insert(NormalizedTermValue::from("hello"), test_nterm);

        let matches = find_matches("hello world", &thesaurus, false).unwrap();

        // Should only find "hello", not empty pattern matches
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].term, "hello");
    }

    #[test]
    fn test_find_matches_golden_terms_and_positions() {
        let mut thesaurus = Thesaurus::new("golden".to_string());
        for (id, term) in [(1u64, "rust"), (2, "async"), (3, "tokio")] {
            let nv = NormalizedTermValue::from(term);
            thesaurus.insert(nv.clone(), NormalizedTerm::new(id, nv));
        }
        let text = "Today I am learning rust async with tokio and more rust";
        let matches = find_matches(text, &thesaurus, true).unwrap();
        let golden: Vec<(String, u64, usize, usize)> = matches
            .iter()
            .map(|m| {
                let (s, e) = m.pos.expect("positions requested");
                (m.term.clone(), m.normalized_term.id, s, e)
            })
            .collect();
        assert_eq!(
            golden,
            vec![
                ("rust".to_string(), 1, 20, 24),
                ("async".to_string(), 2, 25, 30),
                ("tokio".to_string(), 3, 36, 41),
                ("rust".to_string(), 1, 51, 55),
            ]
        );
    }
}
