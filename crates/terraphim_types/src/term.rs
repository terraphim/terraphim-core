//! Term domain: normalised term values, normalised terms and concepts.

use serde::{Deserialize, Serialize};
use std::fmt::{self, Display, Formatter};
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(feature = "typescript")]
use tsify::Tsify;

use crate::validation::stable_id;

static INT_SEQ: AtomicU64 = AtomicU64::new(1);
fn get_int_id() -> u64 {
    INT_SEQ.fetch_add(1, Ordering::SeqCst)
}

/// The value of a normalized term
///
/// This is a string that has been normalized to lowercase and trimmed.
#[derive(Default, Debug, Deserialize, Serialize, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "typescript", derive(Tsify))]
#[cfg_attr(feature = "typescript", tsify(into_wasm_abi, from_wasm_abi))]
pub struct NormalizedTermValue(String);

impl NormalizedTermValue {
    /// Creates a new value by trimming whitespace and lowercasing `term`.
    pub fn new(term: String) -> Self {
        let value = term.trim().to_lowercase();
        Self(value)
    }

    /// Returns the normalised term as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for NormalizedTermValue {
    fn from(term: String) -> Self {
        Self::new(term)
    }
}

impl From<&str> for NormalizedTermValue {
    fn from(term: &str) -> Self {
        Self::new(term.to_string())
    }
}

impl Display for NormalizedTermValue {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl AsRef<[u8]> for NormalizedTermValue {
    fn as_ref(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

/// A normalized term is a higher-level term that has been normalized
///
/// It holds a unique identifier to an underlying and the normalized value.
/// The `display_value` field stores the original case for output purposes.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct NormalizedTerm {
    /// Unique identifier for the normalized term (u64)
    pub id: u64,
    /// The normalized value (lowercase, used for case-insensitive matching)
    // This field is currently called `nterm` in the JSON
    #[serde(rename = "nterm")]
    pub value: NormalizedTermValue,
    /// The display value with original case preserved (used for replacement output)
    /// Falls back to `value` if None for backward compatibility
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_value: Option<String>,
    /// The URL of the normalized term
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// CLI action template with `{{ model }}` and `{{ prompt }}` placeholders.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    /// Routing tiebreaking priority (higher = preferred).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<u8>,
    /// Pattern or alias that activates this term.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger: Option<String>,
    /// Whether the term is pinned.
    #[serde(default)]
    pub pinned: bool,
}

impl NormalizedTerm {
    /// Create a new normalized term with the given id and value.
    /// The display_value will be None (falls back to value for output).
    pub fn new(id: u64, value: NormalizedTermValue) -> Self {
        Self {
            id,
            value,
            display_value: None,
            url: None,
            action: None,
            priority: None,
            trigger: None,
            pinned: false,
        }
    }

    /// Create a new normalized term with auto-generated ID.
    /// The display_value will be None (falls back to value for output).
    pub fn with_auto_id(value: NormalizedTermValue) -> Self {
        Self {
            id: get_int_id(),
            value,
            display_value: None,
            url: None,
            action: None,
            priority: None,
            trigger: None,
            pinned: false,
        }
    }

    /// Create a new normalized term with a deterministic, content-derived ID.
    ///
    /// Unlike [`NormalizedTerm::with_auto_id`], the resulting ID is stable
    /// across insertion order, save/reload cycles and process restarts,
    /// because it is derived from the normalised value itself (see
    /// [`stable_id`]) rather than a process-local counter.
    pub fn with_stable_id(value: NormalizedTermValue) -> Self {
        Self {
            id: stable_id(value.as_str()),
            value,
            display_value: None,
            url: None,
            action: None,
            priority: None,
            trigger: None,
            pinned: false,
        }
    }

    /// Set the display value (original case for output).
    /// Use this to preserve the original case from markdown headings.
    pub fn with_display_value(mut self, display_value: String) -> Self {
        self.display_value = Some(display_value);
        self
    }

    /// Set the URL for this term.
    pub fn with_url(mut self, url: String) -> Self {
        self.url = Some(url);
        self
    }

    /// Set the action template for this term.
    pub fn with_action(mut self, action: String) -> Self {
        self.action = Some(action);
        self
    }

    /// Set the priority for this term.
    pub fn with_priority(mut self, priority: u8) -> Self {
        self.priority = Some(priority);
        self
    }

    /// Set the trigger for this term.
    pub fn with_trigger(mut self, trigger: String) -> Self {
        self.trigger = Some(trigger);
        self
    }

    /// Set the pinned flag for this term.
    pub fn with_pinned(mut self, pinned: bool) -> Self {
        self.pinned = pinned;
        self
    }

    /// Get the display value, falling back to the normalized value if not set.
    /// This is the value that should be used for replacement output.
    pub fn display(&self) -> &str {
        self.display_value
            .as_deref()
            .unwrap_or_else(|| self.value.as_str())
    }

    /// Get the action template.
    pub fn action(&self) -> Option<&String> {
        self.action.as_ref()
    }

    /// Get the priority.
    pub fn priority(&self) -> Option<&u8> {
        self.priority.as_ref()
    }

    /// Get the trigger.
    pub fn trigger(&self) -> Option<&String> {
        self.trigger.as_ref()
    }

    /// Get the pinned flag.
    pub fn pinned(&self) -> bool {
        self.pinned
    }
}

/// A concept is a higher-level, normalized term.
///
/// It describes a unique, abstract idea in a machine-readable format.
///
/// An example of a concept is "machine learning" which is normalized from
/// "Machine Learning"
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Concept {
    /// A unique identifier for the concept (u64)
    pub id: u64,
    /// The normalized concept
    pub value: NormalizedTermValue,
}

impl Concept {
    /// Create a new concept with auto-generated ID.
    pub fn new(value: NormalizedTermValue) -> Self {
        Self {
            id: get_int_id(),
            value,
        }
    }

    /// Create a new concept with a deterministic, content-derived ID.
    ///
    /// The ID is stable across processes and rebuilds, unlike
    /// [`Concept::new`], whose counter depends on runtime state.
    pub fn with_stable_id(value: NormalizedTermValue) -> Self {
        Self {
            id: stable_id(value.as_str()),
            value,
        }
    }

    /// Create a new concept with a specific ID.
    pub fn with_id(id: u64, value: NormalizedTermValue) -> Self {
        Self { id, value }
    }
}

impl From<String> for Concept {
    fn from(concept: String) -> Self {
        let concept = NormalizedTermValue::new(concept);
        Self::new(concept)
    }
}

impl Display for Concept {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.value)
    }
}
