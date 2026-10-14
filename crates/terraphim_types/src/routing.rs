//! Routing domain: priority, routing rules, matches and decisions.

use serde::{Deserialize, Serialize};
use std::fmt;

use schemars::JsonSchema;
#[cfg(feature = "typescript")]
use tsify::Tsify;

// Routing and Priority Types

/// Priority level for routing rules and decisions
/// Higher numeric values indicate higher priority
#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, JsonSchema, Default,
)]
#[cfg_attr(feature = "typescript", derive(Tsify))]
#[cfg_attr(feature = "typescript", tsify(into_wasm_abi, from_wasm_abi))]
/// A clamped priority value in the range 0–100 (higher = more urgent).
pub struct Priority(pub u8);

impl Priority {
    /// Create a new priority with the given value
    pub fn new(value: u8) -> Self {
        Self(value.clamp(0, 100))
    }

    /// Get the priority value
    pub fn value(&self) -> u8 {
        self.0
    }

    /// Check if this is high priority (>= 80)
    pub fn is_high(&self) -> bool {
        self.0 >= 80
    }

    /// Check if this is medium priority (>= 40 && < 80)
    pub fn is_medium(&self) -> bool {
        self.0 >= 40 && self.0 < 80
    }

    /// Check if this is low priority (< 40)
    pub fn is_low(&self) -> bool {
        self.0 < 40
    }

    /// Maximum priority value
    pub const MAX: Self = Self(100);

    /// High priority (default for fast/expensive rules)
    pub const HIGH: Self = Self(80);

    /// Medium priority (default for standard rules)
    pub const MEDIUM: Self = Self(50);

    /// Low priority (default for fallback rules)
    pub const LOW: Self = Self(20);

    /// Minimum priority value
    pub const MIN: Self = Self(0);
}

impl fmt::Display for Priority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u8> for Priority {
    fn from(value: u8) -> Self {
        Self::new(value)
    }
}

impl From<i32> for Priority {
    fn from(value: i32) -> Self {
        Self::new(value as u8)
    }
}

/// A routing rule with pattern matching and priority
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "typescript", derive(Tsify))]
#[cfg_attr(feature = "typescript", tsify(into_wasm_abi, from_wasm_abi))]
pub struct RoutingRule {
    /// Unique identifier for this rule
    pub id: String,

    /// Name of the rule (human-readable)
    pub name: String,

    /// Pattern to match (can be regex, exact string, or concept name)
    ///
    /// Kept for backwards compatibility. When a rule carries multiple
    /// patterns, this holds the primary (first) pattern and [`RoutingRule::patterns`]
    /// holds the full list. Invariant: if `patterns` is non-empty then
    /// `patterns[0] == pattern`.
    pub pattern: String,

    /// All patterns this rule matches (concept name plus synonyms, regexes, ...).
    ///
    /// Introduced in 1.22.0 so a rule can group a concept with its synonyms
    /// without emitting one rule per pattern. Normalised by the constructors
    /// and [`RoutingRule::with_patterns`]: always contains `pattern` as the
    /// first element when non-empty.
    #[serde(default)]
    pub patterns: Vec<String>,

    /// Priority of this rule (higher = more important)
    pub priority: Priority,

    /// Provider to route to when this rule matches
    pub provider: String,

    /// Model to use when this rule matches
    pub model: String,

    /// Optional description of when this rule applies
    pub description: Option<String>,

    /// Tags for categorizing rules
    pub tags: Vec<String>,

    /// Whether this rule is enabled
    pub enabled: bool,

    /// When this rule was created
    pub created_at: chrono::DateTime<chrono::Utc>,

    /// When this rule was last updated
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl RoutingRule {
    /// Create a new routing rule
    pub fn new(
        id: String,
        name: String,
        pattern: String,
        priority: Priority,
        provider: String,
        model: String,
    ) -> Self {
        let now = chrono::Utc::now();
        Self {
            id,
            name,
            patterns: vec![pattern.clone()],
            pattern,
            priority,
            provider,
            model,
            description: None,
            tags: Vec::new(),
            enabled: true,
            created_at: now,
            updated_at: now,
        }
    }

    /// Create a rule that matches multiple patterns (e.g. a concept plus its
    /// synonyms). The first pattern is also stored in [`RoutingRule::pattern`].
    pub fn new_multi(
        id: String,
        name: String,
        patterns: Vec<String>,
        priority: Priority,
        provider: String,
        model: String,
    ) -> Self {
        let now = chrono::Utc::now();
        let pattern = patterns.first().cloned().unwrap_or_default();
        Self {
            id,
            name,
            patterns,
            pattern,
            priority,
            provider,
            model,
            description: None,
            tags: Vec::new(),
            enabled: true,
            created_at: now,
            updated_at: now,
        }
    }

    /// Replace the rule's pattern list. The first entry becomes [`RoutingRule::pattern`].
    pub fn with_patterns(mut self, patterns: Vec<String>) -> Self {
        self.pattern = patterns.first().cloned().unwrap_or_default();
        self.patterns = patterns;
        self
    }

    /// Create a rule with default medium priority
    pub fn with_defaults(
        id: String,
        name: String,
        pattern: String,
        provider: String,
        model: String,
    ) -> Self {
        Self::new(id, name, pattern, Priority::MEDIUM, provider, model)
    }

    /// Set the description
    pub fn with_description(mut self, description: String) -> Self {
        self.description = Some(description);
        self
    }

    /// Add a tag
    pub fn with_tag(mut self, tag: String) -> Self {
        self.tags.push(tag);
        self
    }

    /// Set enabled status
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Update the rule's timestamp
    pub fn touch(&mut self) {
        self.updated_at = chrono::Utc::now();
    }
}

/// Result of pattern matching with priority scoring
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "typescript", derive(Tsify))]
#[cfg_attr(feature = "typescript", tsify(into_wasm_abi, from_wasm_abi))]
pub struct PatternMatch {
    /// The concept that was matched
    pub concept: String,

    /// Provider to route to
    pub provider: String,

    /// Model to use
    pub model: String,

    /// Match score (0.0 to 1.0)
    pub score: f64,

    /// Priority of the matched rule
    pub priority: Priority,

    /// Combined weighted score (score * priority_factor)
    pub weighted_score: f64,

    /// The rule that was matched
    pub rule_id: String,
}

impl PatternMatch {
    /// Create a new pattern match
    pub fn new(
        concept: String,
        provider: String,
        model: String,
        score: f64,
        priority: Priority,
        rule_id: String,
    ) -> Self {
        let priority_factor = priority.value() as f64 / 100.0;
        let weighted_score = score * priority_factor;

        Self {
            concept,
            provider,
            model,
            score,
            priority,
            weighted_score,
            rule_id,
        }
    }

    /// Create a simple pattern match with default priority
    pub fn simple(concept: String, provider: String, model: String, score: f64) -> Self {
        Self::new(
            concept,
            provider,
            model,
            score,
            Priority::MEDIUM,
            "default".to_string(),
        )
    }
}

/// Routing decision with priority information
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "typescript", derive(Tsify))]
#[cfg_attr(feature = "typescript", tsify(into_wasm_abi, from_wasm_abi))]
pub struct RoutingDecision {
    /// Provider to route to
    pub provider: String,

    /// Model to use
    pub model: String,

    /// The scenario that was matched
    pub scenario: RoutingScenario,

    /// Priority of this decision
    pub priority: Priority,

    /// Confidence score (0.0 to 1.0)
    pub confidence: f64,

    /// The rule that led to this decision (if any)
    pub rule_id: Option<String>,

    /// Reason for this decision
    pub reason: String,
}

impl RoutingDecision {
    /// Create a new routing decision
    pub fn new(
        provider: String,
        model: String,
        scenario: RoutingScenario,
        priority: Priority,
        confidence: f64,
        reason: String,
    ) -> Self {
        Self {
            provider,
            model,
            scenario,
            priority,
            confidence,
            rule_id: None,
            reason,
        }
    }

    /// Create a decision with a specific rule
    pub fn with_rule(
        provider: String,
        model: String,
        scenario: RoutingScenario,
        priority: Priority,
        confidence: f64,
        rule_id: String,
        reason: String,
    ) -> Self {
        Self {
            provider,
            model,
            scenario,
            priority,
            confidence,
            rule_id: Some(rule_id),
            reason,
        }
    }

    /// Create a simple default decision
    pub fn default(provider: String, model: String) -> Self {
        Self::new(
            provider,
            model,
            RoutingScenario::Default,
            Priority::LOW,
            0.5,
            "Default routing".to_string(),
        )
    }
}

/// Routing scenario types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, Default)]
#[cfg_attr(feature = "typescript", derive(Tsify))]
#[cfg_attr(feature = "typescript", tsify(into_wasm_abi, from_wasm_abi))]
pub enum RoutingScenario {
    /// Default routing scenario
    #[serde(rename = "default")]
    #[default]
    Default,

    /// Background processing (low priority, cost-optimized)
    #[serde(rename = "background")]
    Background,

    /// Thinking/reasoning tasks (high quality)
    #[serde(rename = "think")]
    Think,

    /// Long context tasks
    #[serde(rename = "long_context")]
    LongContext,

    /// Web search required
    #[serde(rename = "web_search")]
    WebSearch,

    /// Image processing required
    #[serde(rename = "image")]
    Image,

    /// Pattern-based routing with concept name
    #[serde(rename = "pattern")]
    Pattern(String),

    /// Priority-based routing
    #[serde(rename = "priority")]
    Priority,

    /// Custom scenario
    #[serde(rename = "custom")]
    Custom(String),
}

impl fmt::Display for RoutingScenario {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Default => write!(f, "default"),
            Self::Background => write!(f, "background"),
            Self::Think => write!(f, "think"),
            Self::LongContext => write!(f, "long_context"),
            Self::WebSearch => write!(f, "web_search"),
            Self::Image => write!(f, "image"),
            Self::Pattern(concept) => write!(f, "pattern:{}", concept),
            Self::Priority => write!(f, "priority"),
            Self::Custom(name) => write!(f, "custom:{}", name),
        }
    }
}
