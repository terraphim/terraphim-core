//! Role domain: user profile / persona naming types.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;

use schemars::JsonSchema;
#[cfg(feature = "typescript")]
use tsify::Tsify;

/// A role name with case-insensitive lookup support.
///
/// Stores both the original casing and a lowercase version for efficient
/// case-insensitive operations. Roles represent different user profiles or
/// personas in the Terraphim system, each with specific knowledge domains
/// and search preferences.
///
/// Note: Equality is based on both fields, so two instances with different
/// original casing are not equal. Use `as_lowercase()` for case-insensitive comparisons.
///
/// # Examples
///
/// ```
/// use terraphim_types::RoleName;
///
/// let role = RoleName::new("DataScientist");
/// assert_eq!(role.as_str(), "DataScientist");
/// assert_eq!(role.as_lowercase(), "datascientist");
///
/// // Compare using lowercase for case-insensitive matching
/// let role2 = RoleName::new("datascientist");
/// assert_eq!(role.as_lowercase(), role2.as_lowercase());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, JsonSchema)]
#[cfg_attr(feature = "typescript", derive(Tsify))]
#[cfg_attr(feature = "typescript", tsify(into_wasm_abi, from_wasm_abi))]
pub struct RoleName {
    /// The original role name preserving the original casing
    pub original: String,
    /// Lowercase version for case-insensitive comparisons
    pub lowercase: String,
}

impl RoleName {
    /// Creates a new role name from a string.
    ///
    /// # Arguments
    ///
    /// * `name` - The role name with any casing
    ///
    /// # Examples
    ///
    /// ```
    /// use terraphim_types::RoleName;
    ///
    /// let role = RoleName::new("SoftwareEngineer");
    /// ```
    pub fn new(name: &str) -> Self {
        RoleName {
            original: name.to_string(),
            lowercase: name.to_lowercase(),
        }
    }

    /// Returns the lowercase version of the role name.
    ///
    /// Use this for case-insensitive comparisons.
    pub fn as_lowercase(&self) -> &str {
        &self.lowercase
    }

    /// Returns the original role name with preserved casing.
    pub fn as_str(&self) -> &str {
        &self.original
    }
}

impl fmt::Display for RoleName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.original)
    }
}

impl FromStr for RoleName {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(RoleName::new(s))
    }
}

impl From<&str> for RoleName {
    fn from(s: &str) -> Self {
        RoleName::new(s)
    }
}

impl From<String> for RoleName {
    fn from(s: String) -> Self {
        RoleName::new(&s)
    }
}

impl Serialize for RoleName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.original)
    }
}

impl<'de> Deserialize<'de> for RoleName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(RoleName::new(&s))
    }
}
