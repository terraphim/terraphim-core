//! Contract tests for bounded, lossless markdown parsing (issue #38).
//!
//! Strict parsing must fail with typed errors rather than return silently
//! truncated knowledge; recovering parsing must report diagnostics and mark
//! output as incomplete whenever loss occurs.

use terraphim_markdown_parser::{
    LimitKind, MarkdownParserError, ParseLimits, ParseLimitsError, parse_recovering, parse_strict,
};

fn default_limits() -> ParseLimits {
    ParseLimits::default()
}

#[test]
fn strict_parses_valid_document() {
    let md = "# Title\n\nHello world\n\n- one\n- two\n";
    let out = parse_strict(md, &default_limits()).unwrap();
    assert!(!out.blocks.is_empty());
    assert!(out.markdown.contains("Hello world"));
}

#[test]
fn strict_rejects_oversized_input() {
    let limits = ParseLimits::default().with_max_input_bytes(64);
    let md = "x".repeat(65);
    let err = parse_strict(&md, &limits).unwrap_err();
    match err {
        MarkdownParserError::LimitExceeded(LimitKind::InputBytes, limit, actual) => {
            assert_eq!(limit, 64);
            assert_eq!(actual, 65);
        }
        other => panic!("expected InputBytes limit, got {other:?}"),
    }
}

#[test]
fn strict_accepts_input_at_limit_boundary() {
    let limits = ParseLimits::default().with_max_input_bytes(64);
    let md = "x".repeat(64);
    parse_strict(&md, &limits).unwrap();
}

#[test]
fn strict_rejects_excessive_line_length() {
    let limits = ParseLimits::default().with_max_line_length(32);
    let md = format!("{}\n", "y".repeat(33));
    let err = parse_strict(&md, &limits).unwrap_err();
    assert!(matches!(
        err,
        MarkdownParserError::LimitExceeded(LimitKind::LineLength, 32, 33)
    ));
}

#[test]
fn strict_rejects_deep_nesting() {
    let limits = ParseLimits::default().with_max_depth(8);
    let mut md = String::new();
    for i in 0..40 {
        md.push_str(&" ".repeat(i * 2));
        md.push_str("- item\n");
    }
    let err = parse_strict(&md, &limits).unwrap_err();
    assert!(matches!(
        err,
        MarkdownParserError::LimitExceeded(LimitKind::NestingDepth, 8, _)
    ));
}

#[test]
fn strict_rejects_node_count_exceeded() {
    let limits = ParseLimits::default().with_max_nodes(10);
    let md = "a\n\nb\n\nc\n\nd\n\ne\n\nf\n\ng\n\nh\n\ni\n\nj\n\nk\n\nl\n";
    let err = parse_strict(md, &limits).unwrap_err();
    assert!(matches!(
        err,
        MarkdownParserError::LimitExceeded(LimitKind::NodeCount, 10, _)
    ));
}

#[test]
fn strict_rejects_oversized_output() {
    // Deeply repeated block-id insertion blows past a tiny output budget.
    let limits = ParseLimits::default().with_max_output_bytes(64);
    let md = "para one\n\npara two\n\npara three\n\npara four\n";
    let err = parse_strict(md, &limits).unwrap_err();
    assert!(matches!(
        err,
        MarkdownParserError::LimitExceeded(LimitKind::OutputBytes, 64, _)
    ));
}

#[test]
fn limits_cannot_be_raised_by_clamping_only() {
    // Clamp only lowers; raising requires explicit `with_*`.
    let limits = ParseLimits::default().clamp_max_input_bytes(1);
    let err = parse_strict("ok", &limits).unwrap_err();
    assert!(matches!(
        err,
        MarkdownParserError::LimitExceeded(LimitKind::InputBytes, 1, 2)
    ));
}

#[test]
fn recovering_reports_incomplete_on_limit() {
    let limits = ParseLimits::default().with_max_input_bytes(8);
    let report = parse_recovering("a much longer document than eight bytes", &limits).unwrap();
    assert!(!report.complete);
    assert!(!report.diagnostics.is_empty());
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.limit == Some(LimitKind::InputBytes))
    );
}

#[test]
fn recovering_reports_complete_for_valid_document() {
    let report = parse_recovering("# Fine\n\nbody\n", &default_limits()).unwrap();
    assert!(report.complete);
    assert!(report.diagnostics.is_empty());
}

#[test]
fn recovering_preserves_plain_paragraphs() {
    // Normalization inserts block ids, so a plain paragraph is not a loss case.
    let report = parse_recovering("plain paragraph\n", &default_limits()).unwrap();
    assert!(report.complete);
    assert!(report.normalized.markdown.contains("plain paragraph"));
}

#[test]
fn strict_never_returns_partial_output_on_error() {
    let limits = ParseLimits::default().with_max_output_bytes(32);
    let err = parse_strict("para\n\npara\n\npara\n", &limits);
    assert!(err.is_err());
}

#[test]
fn empty_input_and_bom_are_accepted() {
    parse_strict("", &default_limits()).unwrap();
    parse_strict("\u{feff}# Bom title\n", &default_limits()).unwrap();
}

#[test]
fn mixed_line_endings_accepted() {
    let md = "# T\r\n\rbody\r\n- item\n";
    parse_strict(md, &default_limits()).unwrap();
}

#[test]
fn multibyte_content_counts_bytes_not_chars() {
    let limits = ParseLimits::default().with_max_line_length(4);
    // Each 'é' is two bytes, so a five-character line is ten bytes.
    let md = "ééééé\n";
    let err = parse_strict(md, &limits).unwrap_err();
    assert!(matches!(
        err,
        MarkdownParserError::LimitExceeded(LimitKind::LineLength, 4, 10)
    ));
}

#[test]
fn limits_error_is_displayable() {
    let e = ParseLimitsError::LineTooLong {
        limit: 1,
        actual: 2,
    };
    assert!(e.to_string().contains("line"));
}
