use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[cfg(feature = "fs-traversal")]
use std::fs;

#[cfg(feature = "fs-traversal")]
use walkdir::WalkDir;

use terraphim_types::{DocumentType, MarkdownDirectives, RouteDirective};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownDirectiveWarning {
    pub path: PathBuf,
    pub line: Option<usize>,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MarkdownDirectivesParseResult {
    pub directives: HashMap<String, MarkdownDirectives>,
    pub warnings: Vec<MarkdownDirectiveWarning>,
}

/// Parses markdown directives from an in-memory string (portable kernel API).
///
/// Pure, synchronous core: accepts a concept key and markdown content as
/// supplied inputs and performs no I/O. Directory traversal is an adapter
/// concern provided by `parse_markdown_directives_dir` behind the
/// `fs-traversal` feature. Warning `path` fields use a synthetic
/// `<concept>.md` path.
pub fn parse_markdown_directives_str(
    concept: &str,
    content: &str,
) -> MarkdownDirectivesParseResult {
    let path = PathBuf::from(format!("{concept}.md"));
    let mut warnings = Vec::new();
    let doc_directives = parse_markdown_directives_content(&path, content, &mut warnings);
    let mut directives = HashMap::new();
    directives.insert(concept.to_string(), doc_directives);
    MarkdownDirectivesParseResult {
        directives,
        warnings,
    }
}

/// Parses all markdown directive files in `root`, collecting synonyms and warnings.
///
/// Requires the `fs-traversal` adapter feature; without it the result is
/// empty and carries a single explanatory warning.
#[cfg(feature = "fs-traversal")]
pub fn parse_markdown_directives_dir(root: &Path) -> crate::Result<MarkdownDirectivesParseResult> {
    let mut directives = HashMap::new();
    let mut warnings = Vec::new();

    for entry in WalkDir::new(root) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                let path = err.path().unwrap_or(root).to_path_buf();
                warnings.push(MarkdownDirectiveWarning {
                    path,
                    line: None,
                    message: format!("Failed to read directory entry: {}", err),
                });
                continue;
            }
        };

        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        let extension = path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if extension != "md" {
            continue;
        }

        let concept = match path.file_stem().and_then(|stem| stem.to_str()) {
            Some(stem) => stem.to_string(),
            None => {
                warnings.push(MarkdownDirectiveWarning {
                    path: path.to_path_buf(),
                    line: None,
                    message: "Failed to read file stem for concept key".to_string(),
                });
                continue;
            }
        };

        let content = match fs::read_to_string(path) {
            Ok(content) => content,
            Err(err) => {
                warnings.push(MarkdownDirectiveWarning {
                    path: path.to_path_buf(),
                    line: None,
                    message: format!("Failed to read markdown file: {}", err),
                });
                continue;
            }
        };

        let doc_directives = parse_markdown_directives_content(path, &content, &mut warnings);
        directives.insert(concept, doc_directives);
    }

    Ok(MarkdownDirectivesParseResult {
        directives,
        warnings,
    })
}

/// Extract the first `# Heading` from a markdown file at the given path.
///
/// Reads the file and delegates to `terraphim_markdown_parser::extract_first_heading`
/// for proper AST-based heading extraction. Returns `None` if the file cannot be read
/// or has no H1 heading.
#[cfg(feature = "fs-traversal")]
pub fn extract_heading_from_path(path: &Path) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    terraphim_markdown_parser::extract_first_heading(&content)
}

/// Portable kernel fallback: no filesystem access without the adapter feature.
#[cfg(not(feature = "fs-traversal"))]
pub fn extract_heading_from_path(_path: &Path) -> Option<String> {
    None
}

fn parse_markdown_directives_content(
    path: &Path,
    content: &str,
    warnings: &mut Vec<MarkdownDirectiveWarning>,
) -> MarkdownDirectives {
    let mut doc_type: Option<DocumentType> = None;
    let mut explicit_doc_type: Option<DocumentType> = None;
    let mut synonyms: Vec<String> = Vec::new();
    let mut routes: Vec<RouteDirective> = Vec::new();
    let mut priority: Option<u8> = None;
    let mut trigger: Option<String> = None;
    let mut pinned: bool = false;

    // Use AST parser for proper heading extraction (handles inline code, emphasis, etc.)
    let heading = terraphim_markdown_parser::extract_first_heading(content);

    for (idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let lower = trimmed.to_ascii_lowercase();
        if lower.starts_with("type:::") {
            if doc_type.is_some() {
                continue;
            }
            let value = trimmed["type:::".len()..].trim();
            let value_lower = value.to_ascii_lowercase();
            let parsed = match value_lower.as_str() {
                "kg_entry" => Some(DocumentType::KgEntry),
                "document" => Some(DocumentType::Document),
                "config_document" => Some(DocumentType::ConfigDocument),
                _ => None,
            };

            match parsed {
                Some(parsed) => {
                    doc_type = Some(parsed);
                    explicit_doc_type = Some(parsed);
                }
                None => warnings.push(MarkdownDirectiveWarning {
                    path: path.to_path_buf(),
                    line: Some(idx + 1),
                    message: format!("Invalid type directive '{}'", value),
                }),
            }
            continue;
        }

        if lower.starts_with("synonyms::") {
            let value = trimmed["synonyms::".len()..].trim();
            for raw in value.split(',') {
                let normalized = raw.trim().to_ascii_lowercase();
                if !normalized.is_empty() {
                    synonyms.push(normalized);
                }
            }
            continue;
        }

        if (lower.starts_with("route::") || lower.starts_with("routing::"))
            && explicit_doc_type != Some(DocumentType::Document)
        {
            let prefix_len = if lower.starts_with("route::") {
                "route::".len()
            } else {
                "routing::".len()
            };
            let value = trimmed[prefix_len..].trim();
            let mut parts = value.splitn(2, ',');
            let provider = parts.next().unwrap_or("").trim();
            let model = parts.next().unwrap_or("").trim();

            if provider.is_empty() || model.is_empty() {
                warnings.push(MarkdownDirectiveWarning {
                    path: path.to_path_buf(),
                    line: Some(idx + 1),
                    message: format!("Invalid route directive '{}'", value),
                });
            } else {
                routes.push(RouteDirective {
                    provider: provider.to_ascii_lowercase(),
                    model: model.to_string(),
                    action: None,
                    is_free: false,
                });
            }
            continue;
        }

        if lower.starts_with("action::") && explicit_doc_type != Some(DocumentType::Document) {
            let value = trimmed["action::".len()..].trim();
            if !value.is_empty() {
                // Attach action to the most recently parsed route
                if let Some(last_route) = routes.last_mut() {
                    last_route.action = Some(value.to_string());
                } else {
                    warnings.push(MarkdownDirectiveWarning {
                        path: path.to_path_buf(),
                        line: Some(idx + 1),
                        message: "action:: directive without a preceding route:: directive"
                            .to_string(),
                    });
                }
            }
            continue;
        }

        if (lower.starts_with("is_free::") || lower.starts_with("free::"))
            && explicit_doc_type != Some(DocumentType::Document)
        {
            let value = trimmed[lower.find("::").unwrap() + 2..]
                .trim()
                .to_ascii_lowercase();
            let is_free = matches!(value.as_str(), "true" | "yes" | "1");
            if let Some(last_route) = routes.last_mut() {
                last_route.is_free = is_free;
            } else {
                warnings.push(MarkdownDirectiveWarning {
                    path: path.to_path_buf(),
                    line: Some(idx + 1),
                    message: "is_free:: directive without a preceding route:: directive"
                        .to_string(),
                });
            }
            continue;
        }

        if lower.starts_with("priority::") {
            if priority.is_some() {
                continue;
            }
            let value = trimmed["priority::".len()..].trim();
            match value.parse::<u8>() {
                Ok(parsed) if parsed <= 100 => priority = Some(parsed),
                _ => warnings.push(MarkdownDirectiveWarning {
                    path: path.to_path_buf(),
                    line: Some(idx + 1),
                    message: format!("Invalid priority directive '{}'", value),
                }),
            }
            continue;
        }

        if lower.starts_with("trigger::") {
            if trigger.is_some() {
                continue; // First trigger wins
            }
            let value = trimmed["trigger::".len()..].trim();
            if !value.is_empty() {
                trigger = Some(value.to_string());
            }
            continue;
        }

        if lower.starts_with("pinned::") {
            let value = trimmed["pinned::".len()..].trim().to_ascii_lowercase();
            pinned = matches!(value.as_str(), "true" | "yes" | "1");
            continue;
        }
    }

    // Primary route is the first in the list (backward compatible)
    let route = routes.first().cloned();

    let doc_type = doc_type.unwrap_or_else(|| {
        if route.is_some() {
            DocumentType::ConfigDocument
        } else {
            DocumentType::KgEntry
        }
    });

    MarkdownDirectives {
        doc_type,
        synonyms,
        route,
        routes,
        priority,
        trigger,
        pinned,
        heading,
    }
}

#[cfg(test)]
mod tests {
    // Tests for the in-memory kernel pull in all public symbols of the
    // module. Filesystem-backed tests additionally need `DocumentType`,
    // `RouteDirective`, and `parse_markdown_directives_dir`; gating on
    // `fs-traversal` keeps the kernel-only path free of unused imports.
    #[cfg(feature = "fs-traversal")]
    use super::*;

    #[cfg(feature = "fs-traversal")]
    use tempfile::tempdir;

    /// Kernel-level tests for the pure, in-memory directive parser. These run
    /// with and without the `fs-traversal` adapter feature (Refs #31).
    mod str_parser {
        use super::super::*;

        #[test]
        fn parses_synonyms_from_string() {
            let result = parse_markdown_directives_str(
                "alpha",
                "synonyms:: Alpha, beta\nsynonyms:: gamma\n\nSome content",
            );
            let directives = result.directives.get("alpha").unwrap();
            assert!(result.warnings.is_empty());
            assert_eq!(
                directives.synonyms,
                vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()]
            );
        }

        #[test]
        fn parses_route_priority_trigger_pinned_from_string() {
            let content = "\
type::: kg_entry
route:: anthropic, claude-3-5-sonnet
action:: chat
is_free:: true
priority:: 42
trigger:: when managing dependencies
pinned:: true
";
            let result = parse_markdown_directives_str("routing", content);
            let directives = result.directives.get("routing").unwrap();
            assert!(result.warnings.is_empty(), "{:?}", result.warnings);
            assert_eq!(directives.doc_type, DocumentType::KgEntry);
            let route = directives.route.as_ref().unwrap();
            assert_eq!(route.provider, "anthropic");
            assert_eq!(route.model, "claude-3-5-sonnet");
            assert_eq!(route.action.as_deref(), Some("chat"));
            assert!(route.is_free);
            assert_eq!(directives.priority, Some(42));
            assert_eq!(
                directives.trigger.as_deref(),
                Some("when managing dependencies")
            );
            assert!(directives.pinned);
        }

        #[test]
        fn collects_warnings_for_invalid_directives_from_string() {
            let content = "priority:: not-a-number\nroute:: only-provider\n";
            let result = parse_markdown_directives_str("broken", content);
            let _directives = result.directives.get("broken").unwrap();
            assert_eq!(result.warnings.len(), 2, "{:?}", result.warnings);
        }

        #[test]
        fn empty_content_yields_defaults() {
            let result = parse_markdown_directives_str("empty", "");
            let directives = result.directives.get("empty").unwrap();
            assert!(result.warnings.is_empty());
            assert_eq!(directives.doc_type, DocumentType::KgEntry);
            assert!(directives.synonyms.is_empty());
            assert!(directives.route.is_none());
            assert_eq!(directives.priority, None);
        }
    }

    // Filesystem-backed tests require the fs-traversal adapter feature.
    #[cfg(feature = "fs-traversal")]
    #[test]
    fn parses_synonyms_only() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("alpha.md");
        fs::write(
            &path,
            "synonyms:: Alpha, beta\nsynonyms:: gamma\n\nSome content",
        )
        .unwrap();

        let result = parse_markdown_directives_dir(dir.path()).unwrap();
        let directives = result.directives.get("alpha").unwrap();
        assert_eq!(directives.doc_type, DocumentType::KgEntry);
        assert_eq!(
            directives.synonyms,
            vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()]
        );
        assert!(directives.route.is_none());
        assert!(directives.priority.is_none());
        assert!(result.warnings.is_empty());
    }

    #[cfg(feature = "fs-traversal")]
    #[test]
    fn parses_config_route_priority() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("plan.md");
        fs::write(
            &path,
            "type::: config_document\nroute:: openai, gpt-4o\npriority:: 80",
        )
        .unwrap();

        let result = parse_markdown_directives_dir(dir.path()).unwrap();
        let directives = result.directives.get("plan").unwrap();
        assert_eq!(directives.doc_type, DocumentType::ConfigDocument);
        assert_eq!(
            directives.route,
            Some(RouteDirective {
                provider: "openai".to_string(),
                model: "gpt-4o".to_string(),
                action: None,
                is_free: false,
            })
        );
        assert_eq!(directives.priority, Some(80));
        assert!(directives.synonyms.is_empty());
        assert!(result.warnings.is_empty());
    }

    #[cfg(feature = "fs-traversal")]
    #[test]
    fn infers_config_document_when_route_present() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("route_only.md");
        fs::write(&path, "route:: anthropic, claude-3-5-sonnet").unwrap();

        let result = parse_markdown_directives_dir(dir.path()).unwrap();
        let directives = result.directives.get("route_only").unwrap();
        assert_eq!(directives.doc_type, DocumentType::ConfigDocument);
        assert!(directives.route.is_some());
        assert!(result.warnings.is_empty());
    }

    #[cfg(feature = "fs-traversal")]
    #[test]
    fn parses_is_free_directive() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("free.md"),
            "route:: zai, glm-5-turbo\nis_free:: true\naction:: opencode -m {{ model }}\n",
        )
        .unwrap();

        let result = parse_markdown_directives_dir(dir.path()).unwrap();
        let directives = result.directives.get("free").unwrap();
        let route = directives.routes.first().unwrap();
        assert!(route.is_free);
        assert_eq!(route.provider, "zai");
        assert_eq!(route.model, "glm-5-turbo");
    }

    #[cfg(feature = "fs-traversal")]
    #[test]
    fn parses_free_alias_directive() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("free.md"),
            "route:: zai, glm-5-turbo\nfree:: yes\n",
        )
        .unwrap();

        let result = parse_markdown_directives_dir(dir.path()).unwrap();
        let directives = result.directives.get("free").unwrap();
        let route = directives.routes.first().unwrap();
        assert!(route.is_free);
    }

    #[cfg(feature = "fs-traversal")]
    #[test]
    fn is_free_defaults_to_false() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("paid.md"), "route:: anthropic, sonnet\n").unwrap();

        let result = parse_markdown_directives_dir(dir.path()).unwrap();
        let directives = result.directives.get("paid").unwrap();
        let route = directives.routes.first().unwrap();
        assert!(!route.is_free);
    }

    #[cfg(feature = "fs-traversal")]
    #[test]
    fn warns_on_invalid_priority() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bad_priority.md");
        fs::write(&path, "priority:: 200").unwrap();

        let result = parse_markdown_directives_dir(dir.path()).unwrap();
        let directives = result.directives.get("bad_priority").unwrap();
        assert!(directives.priority.is_none());
        assert_eq!(result.warnings.len(), 1);
    }

    #[cfg(feature = "fs-traversal")]
    #[test]
    fn warns_on_invalid_route() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bad_route.md");
        fs::write(&path, "route:: only_provider").unwrap();

        let result = parse_markdown_directives_dir(dir.path()).unwrap();
        let directives = result.directives.get("bad_route").unwrap();
        assert!(directives.route.is_none());
        assert_eq!(directives.doc_type, DocumentType::KgEntry);
        assert_eq!(result.warnings.len(), 1);
    }

    #[cfg(feature = "fs-traversal")]
    #[test]
    fn parses_trigger_directive() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.md");
        fs::write(&path, "trigger:: when managing dependencies").unwrap();

        let result = parse_markdown_directives_dir(dir.path()).unwrap();
        let directives = result.directives.get("test").unwrap();
        assert_eq!(
            directives.trigger,
            Some("when managing dependencies".to_string())
        );
        assert!(result.warnings.is_empty());
    }

    #[cfg(feature = "fs-traversal")]
    #[test]
    fn parses_pinned_directive() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.md");
        fs::write(&path, "pinned:: true").unwrap();

        let result = parse_markdown_directives_dir(dir.path()).unwrap();
        let directives = result.directives.get("test").unwrap();
        assert!(directives.pinned);
    }

    #[cfg(feature = "fs-traversal")]
    #[test]
    fn pinned_false_variants() {
        let dir = tempdir().unwrap();

        for (filename, value) in [("false", "false"), ("no", "no"), ("zero", "0")] {
            let path = dir.path().join(format!("{filename}.md"));
            fs::write(&path, format!("pinned:: {value}")).unwrap();
        }

        let result = parse_markdown_directives_dir(dir.path()).unwrap();
        assert!(!result.directives.get("false").unwrap().pinned);
        assert!(!result.directives.get("no").unwrap().pinned);
        assert!(!result.directives.get("zero").unwrap().pinned);
    }

    #[cfg(feature = "fs-traversal")]
    #[test]
    fn trigger_and_synonyms_coexist() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.md");
        fs::write(
            &path,
            "synonyms:: alpha, beta\ntrigger:: when using alphas\n",
        )
        .unwrap();

        let result = parse_markdown_directives_dir(dir.path()).unwrap();
        let directives = result.directives.get("test").unwrap();
        assert_eq!(
            directives.synonyms,
            vec!["alpha".to_string(), "beta".to_string()]
        );
        assert_eq!(directives.trigger, Some("when using alphas".to_string()));
    }

    #[cfg(feature = "fs-traversal")]
    #[test]
    fn empty_trigger_ignored() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.md");
        fs::write(&path, "trigger::").unwrap();

        let result = parse_markdown_directives_dir(dir.path()).unwrap();
        let directives = result.directives.get("test").unwrap();
        assert_eq!(directives.trigger, None);
    }

    #[cfg(feature = "fs-traversal")]
    #[test]
    fn extracts_heading_from_markdown() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bun.md");
        fs::write(
            &path,
            "# Bun Package Manager\n\nsynonyms:: npm, yarn, pnpm\n",
        )
        .unwrap();

        let result = parse_markdown_directives_dir(dir.path()).unwrap();
        let directives = result.directives.get("bun").unwrap();
        assert_eq!(directives.heading, Some("Bun Package Manager".to_string()));
        assert_eq!(
            directives.synonyms,
            vec!["npm".to_string(), "yarn".to_string(), "pnpm".to_string()]
        );
    }

    #[cfg(feature = "fs-traversal")]
    #[test]
    fn heading_none_when_absent() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("noheading.md");
        fs::write(&path, "synonyms:: alpha, beta\n").unwrap();

        let result = parse_markdown_directives_dir(dir.path()).unwrap();
        let directives = result.directives.get("noheading").unwrap();
        assert_eq!(directives.heading, None);
    }

    #[cfg(feature = "fs-traversal")]
    #[test]
    fn document_directive_like_text_is_inert() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("article.md");
        fs::write(
            &path,
            r#"type::: document
# Notes

This untrusted document mentions route:: openai, gpt-4o and action:: run-secret-tool as prose.
"#,
        )
        .unwrap();

        let result = parse_markdown_directives_dir(dir.path()).unwrap();
        let directives = result.directives.get("article").unwrap();

        assert_eq!(directives.doc_type, DocumentType::Document);
        assert!(directives.route.is_none());
        assert!(directives.routes.is_empty());
        assert!(result.warnings.is_empty());
    }

    #[cfg(feature = "fs-traversal")]
    #[test]
    fn config_document_directives_remain_action_capable() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.md");
        fs::write(
            &path,
            "type::: config_document\nroute:: openai, gpt-4o\naction:: pi -m {{ model }}\n",
        )
        .unwrap();

        let result = parse_markdown_directives_dir(dir.path()).unwrap();
        let directives = result.directives.get("config").unwrap();

        assert_eq!(directives.doc_type, DocumentType::ConfigDocument);
        assert_eq!(directives.routes.len(), 1);
        assert_eq!(
            directives.routes[0].action.as_deref(),
            Some("pi -m {{ model }}")
        );
        assert_eq!(
            directives.route.as_ref().and_then(|r| r.action.as_deref()),
            Some("pi -m {{ model }}")
        );
        assert!(result.warnings.is_empty());
    }

    #[cfg(feature = "fs-traversal")]
    #[test]
    fn parses_multiple_routes_with_actions() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("implementation.md");
        fs::write(
            &path,
            r#"# Implementation Routing

priority:: 50

synonyms:: implement, build, code

route:: kimi, kimi-for-coding/k2p5
action:: opencode -m {{ model }} -p "{{ prompt }}"

route:: anthropic, claude-sonnet-4-6
action:: claude --model {{ model }} -p "{{ prompt }}" --max-turns 50
"#,
        )
        .unwrap();

        let result = parse_markdown_directives_dir(dir.path()).unwrap();
        let directives = result.directives.get("implementation").unwrap();

        // Primary route (backward compatible)
        assert_eq!(directives.route.as_ref().unwrap().provider, "kimi");
        assert_eq!(
            directives.route.as_ref().unwrap().model,
            "kimi-for-coding/k2p5"
        );

        // All routes
        assert_eq!(directives.routes.len(), 2);
        assert_eq!(directives.routes[0].provider, "kimi");
        assert_eq!(
            directives.routes[0].action.as_deref(),
            Some(r#"opencode -m {{ model }} -p "{{ prompt }}""#)
        );
        assert_eq!(directives.routes[1].provider, "anthropic");
        assert_eq!(directives.routes[1].model, "claude-sonnet-4-6");
        assert_eq!(
            directives.routes[1].action.as_deref(),
            Some(r#"claude --model {{ model }} -p "{{ prompt }}" --max-turns 50"#)
        );

        assert!(result.warnings.is_empty());
    }

    #[cfg(feature = "fs-traversal")]
    #[test]
    fn action_without_route_warns() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("orphan_action.md");
        fs::write(&path, r#"action:: opencode -m foo -p "{{ prompt }}""#).unwrap();

        let result = parse_markdown_directives_dir(dir.path()).unwrap();
        assert_eq!(result.warnings.len(), 1);
        assert!(
            result.warnings[0]
                .message
                .contains("without a preceding route")
        );
    }

    #[cfg(feature = "fs-traversal")]
    #[test]
    fn extract_heading_from_path_works() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.md");
        fs::write(&path, "# My Heading\n\nSome content\n").unwrap();

        assert_eq!(
            extract_heading_from_path(&path),
            Some("My Heading".to_string())
        );
    }

    #[cfg(feature = "fs-traversal")]
    #[test]
    fn extract_heading_from_path_returns_none_without_heading() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.md");
        fs::write(&path, "Just content\n").unwrap();

        assert_eq!(extract_heading_from_path(&path), None);
    }
}
