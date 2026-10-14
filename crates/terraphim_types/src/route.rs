//! Routing directive types parsed from markdown KG entry front matter.

use serde::{Deserialize, Serialize};

/// Routing directive specifying which LLM provider and model to use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteDirective {
    /// LLM provider name (e.g. `"anthropic"`, `"openrouter"`).
    pub provider: String,
    /// Model identifier within the provider (e.g. `"claude-sonnet-4-5"`).
    pub model: String,
    /// CLI action template with `{{ model }}` and `{{ prompt }}` placeholders.
    #[serde(default)]
    pub action: Option<String>,
    /// Whether this model is free (zero cost).
    #[serde(default)]
    pub is_free: bool,
}

impl RouteDirective {
    /// Extract the CLI basename from the first whitespace-delimited token of
    /// the action template, e.g. `"opencode"` from
    /// `"/home/alex/.bun/bin/opencode run -m {{ model }} ..."`.
    ///
    /// Returns `None` when the action template is missing or empty.
    pub fn cli_basename(&self) -> Option<&str> {
        let first = self.action.as_deref()?.split_whitespace().next()?;
        std::path::Path::new(first).file_name()?.to_str()
    }

    /// Build a probe/health cache key from the route's `(cli, provider, model)`
    /// triple. Used so that the same `(provider, model)` reached via two
    /// different CLIs (e.g. opencode vs. pi-rust) has independent health.
    ///
    /// Returns an empty CLI segment when the action template is missing.
    pub fn route_key(&self) -> String {
        format!(
            "{}:{}:{}",
            self.cli_basename().unwrap_or(""),
            self.provider,
            self.model,
        )
    }
}

#[cfg(test)]
mod route_directive_tests {
    use super::*;

    #[test]
    fn cli_basename_extracts_opencode() {
        let r = RouteDirective {
            provider: "kimi".into(),
            model: "kimi-for-coding/k2p6".into(),
            action: Some(
                "/home/alex/.bun/bin/opencode run -m {{ model }} --format json \"{{ prompt }}\""
                    .into(),
            ),
            is_free: false,
        };
        assert_eq!(r.cli_basename(), Some("opencode"));
    }

    #[test]
    fn cli_basename_extracts_pi_rust() {
        let r = RouteDirective {
            provider: "zai-coding-plan".into(),
            model: "glm-5.1".into(),
            action: Some(
                "/home/alex/.local/bin/pi-rust --provider zai-coding-plan --model {{ model }} -p \"{{ prompt }}\""
                    .into(),
            ),
            is_free: true,
        };
        assert_eq!(r.cli_basename(), Some("pi-rust"));
    }

    #[test]
    fn cli_basename_extracts_claude() {
        let r = RouteDirective {
            provider: "anthropic".into(),
            model: "opus".into(),
            action: Some(
                "/home/alex/.local/bin/claude --model {{ model }} -p \"{{ prompt }}\" --max-turns 50"
                    .into(),
            ),
            is_free: false,
        };
        assert_eq!(r.cli_basename(), Some("claude"));
    }

    #[test]
    fn cli_basename_none_when_action_missing() {
        let r = RouteDirective {
            provider: "x".into(),
            model: "y".into(),
            action: None,
            is_free: false,
        };
        assert_eq!(r.cli_basename(), None);
    }

    #[test]
    fn route_key_distinguishes_cli() {
        let opencode_zai = RouteDirective {
            provider: "zai-coding-plan".into(),
            model: "glm-5.1".into(),
            action: Some("/home/alex/.bun/bin/opencode run -m {{ model }}".into()),
            is_free: true,
        };
        let pi_rust_zai = RouteDirective {
            provider: "zai-coding-plan".into(),
            model: "glm-5.1".into(),
            action: Some("/home/alex/.local/bin/pi-rust --provider zai-coding-plan".into()),
            is_free: true,
        };
        assert_eq!(opencode_zai.route_key(), "opencode:zai-coding-plan:glm-5.1");
        assert_eq!(pi_rust_zai.route_key(), "pi-rust:zai-coding-plan:glm-5.1");
        assert_ne!(opencode_zai.route_key(), pi_rust_zai.route_key());
    }
}
