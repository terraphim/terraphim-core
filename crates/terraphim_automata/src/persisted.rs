//! Versioned persistence for compiled matchers.
//!
//! Every persisted matcher envelope carries a stable format identifier and a
//! format version. Readers dispatch explicitly on the version, migrate
//! supported older versions deterministically, and reject unsupported future
//! versions with a typed error *before* interpreting the payload.
//!
//! Construction configuration ([`MatcherOptions`]) is stored inside the
//! envelope and is re-applied when the matcher is rebuilt on load, so a
//! save-load round trip preserves observable matching behaviour exactly.
//!
//! The envelope is JSON. Options added after format version 1 shipped
//! ([`MatcherOptions::overlap`], [`MatcherOptions::word_boundaries`],
//! [`MatcherOptions::allow_blank_patterns`]) select the version the writer
//! emits:
//!
//! - **Version 1** when all of them keep their defaults
//!   ([`crate::compiled::MatchOverlap::LeftmostLongest`], word boundaries on,
//!   blank patterns rejected). Such a matcher behaves exactly as one built
//!   before the options existed, so the envelope stays fully readable by
//!   older readers, which ignore the extra fields.
//! - **Version 2** when any of them is non-default. An older reader that
//!   only knows version 1 rejects the envelope as an unsupported version,
//!   instead of silently rebuilding a default-mode matcher (a behavioural
//!   downgrade). A 2.0.0 reader knows version 2 but not
//!   `allow_blank_patterns`: it ignores the field, and rebuilding then fails
//!   with [`TerraphimAutomataError::InvalidPattern`] for any whitespace-only
//!   pattern, so it still never produces a matcher missing those patterns.
//!
//! This reader accepts both. In version 1 the newer fields are optional and
//! default to the earlier behaviour; when present they are honoured, so a
//! version 1 envelope carrying non-default values (as written by
//! pre-release builds before version 2 existed) still rebuilds the
//! requested matcher. In version 2 every option field that existed when
//! version 2 shipped is required; `allow_blank_patterns` (added in 2.1.0)
//! is optional and defaults to `false`. Any other version fails with
//! [`TerraphimAutomataError::UnsupportedVersion`].

use std::io::Read;

use serde::{Deserialize, Serialize};
use terraphim_types::NormalizedTerm;

use crate::compiled::{CompiledMatcher, MatchOverlap, MatcherBuilder, MatcherOptions};
use crate::{Result, TerraphimAutomataError};

/// Stable identifier for the compiled-matcher persisted format.
pub const MATCHER_FORMAT_ID: &str = "terraphim.automata.matcher";

/// Newest format version written by this crate.
///
/// Matchers with default overlap and word-boundary options are still written
/// as version 1; see the module documentation.
pub const MATCHER_FORMAT_VERSION: u32 = 2;

/// Construction configuration plus source patterns (format version 1).
///
/// The compiled automaton is deliberately not stored; it is rebuilt from this
/// data on load, applying the persisted options.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PersistedMatcherV1 {
    /// Behaviour-affecting construction options; re-applied on rebuild.
    pub options: MatcherOptions,
    /// Source patterns paired with their normalized terms.
    pub patterns: Vec<(String, NormalizedTerm)>,
}

/// Construction configuration plus source patterns (format version 2).
///
/// Same shape as [`PersistedMatcherV1`], but written only when the options
/// request non-default overlap, word-boundary or blank-pattern behaviour, so
/// readers that predate those options refuse the envelope instead of
/// downgrading it. Every option field is required on read except
/// `allow_blank_patterns`, which defaults to `false`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PersistedMatcherV2 {
    /// Behaviour-affecting construction options; re-applied on rebuild.
    #[serde(deserialize_with = "deserialize_required_options")]
    pub options: MatcherOptions,
    /// Source patterns paired with their normalized terms.
    pub patterns: Vec<(String, NormalizedTerm)>,
}

/// Explicitly dispatched payload versions.
///
/// Each version is a distinct variant; readers never guess.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "version")]
#[non_exhaustive]
pub enum PersistedMatcherPayload {
    /// Initial versioned format; default overlap and word-boundary options.
    #[serde(rename = "1")]
    V1(PersistedMatcherV1),
    /// Non-default overlap, word-boundary or blank-pattern options.
    #[serde(rename = "2")]
    V2(PersistedMatcherV2),
}

impl PersistedMatcherPayload {
    /// The stored construction options, whatever the version.
    pub fn options(&self) -> &MatcherOptions {
        match self {
            Self::V1(data) => &data.options,
            Self::V2(data) => &data.options,
        }
    }

    /// The stored source patterns, whatever the version.
    pub fn patterns(&self) -> &[(String, NormalizedTerm)] {
        match self {
            Self::V1(data) => &data.patterns,
            Self::V2(data) => &data.patterns,
        }
    }

    /// The numeric format version of this payload.
    pub fn version(&self) -> u32 {
        match self {
            Self::V1(_) => 1,
            Self::V2(_) => 2,
        }
    }
}

/// Whether `options` need format version 2 to round-trip safely, that is
/// whether an older (version 1 only) reader would rebuild a different
/// matcher from them.
fn requires_v2(options: &MatcherOptions) -> bool {
    options.overlap != MatchOverlap::LeftmostLongest
        || !options.word_boundaries
        || options.allow_blank_patterns
}

/// Deserialise [`MatcherOptions`] for version 2, where every field is
/// mandatory (version 1 relies on serde defaults for the newer fields).
fn deserialize_required_options<'de, D>(
    deserializer: D,
) -> std::result::Result<MatcherOptions, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    struct RequiredOptions {
        case_insensitive: bool,
        min_pattern_length: usize,
        overlap: MatchOverlap,
        word_boundaries: bool,
        // Added in 2.1.0, after version 2 shipped: optional, so version 2
        // envelopes written by 2.0.0 still load.
        #[serde(default)]
        allow_blank_patterns: bool,
    }
    let raw = RequiredOptions::deserialize(deserializer)?;
    Ok(MatcherOptions::default()
        .with_case_insensitive(raw.case_insensitive)
        .with_min_pattern_length(raw.min_pattern_length)
        .with_overlap(raw.overlap)
        .with_word_boundaries(raw.word_boundaries)
        .with_allow_blank_patterns(raw.allow_blank_patterns))
}

/// The persisted envelope: format identity, version and payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PersistedMatcherEnvelope {
    /// Stable format identifier; see [`MATCHER_FORMAT_ID`].
    pub format_id: String,
    /// Payload, dispatched by explicit version tag.
    #[serde(flatten)]
    pub payload: PersistedMatcherPayload,
}

impl From<&CompiledMatcher> for PersistedMatcherEnvelope {
    fn from(matcher: &CompiledMatcher) -> Self {
        let options = matcher.options().clone();
        let patterns = matcher.patterns().to_vec();
        let payload = if requires_v2(&options) {
            PersistedMatcherPayload::V2(PersistedMatcherV2 { options, patterns })
        } else {
            PersistedMatcherPayload::V1(PersistedMatcherV1 { options, patterns })
        };
        Self {
            format_id: MATCHER_FORMAT_ID.to_string(),
            payload,
        }
    }
}

impl PersistedMatcherEnvelope {
    /// Build an envelope from an existing compiled matcher.
    ///
    /// Uses format version 1 for default overlap, word-boundary and
    /// blank-pattern options
    /// and version 2 otherwise; see the module documentation.
    pub fn from_matcher(matcher: &CompiledMatcher) -> Self {
        Self::from(matcher)
    }

    /// Rebuild a [`CompiledMatcher`] from the envelope, applying the stored
    /// construction options.
    ///
    /// Returns [`TerraphimAutomataError::ConfigurationMismatch`] when the
    /// stored patterns are not valid under the stored options (for example a
    /// pattern shorter than the persisted minimum length).
    pub fn to_matcher(&self) -> Result<CompiledMatcher> {
        let mut builder = MatcherBuilder::new(self.payload.options().clone());
        for (pattern, term) in self.payload.patterns() {
            builder.insert(pattern.clone(), term.clone())?;
        }
        builder.build()
    }

    /// Serialise the envelope to pretty JSON.
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(self).map_err(Into::into)
    }

    /// Parse an envelope from JSON.
    ///
    /// The format identifier is validated first; an unknown identifier or an
    /// unsupported (future) version fails with a typed error before any
    /// payload bytes are interpreted as the current type.
    pub fn from_json(json: &str) -> Result<Self> {
        #[derive(Deserialize)]
        struct Probe {
            format_id: String,
            version: Option<serde_json::Value>,
        }
        let probe: Probe = serde_json::from_str(json).map_err(|err| {
            TerraphimAutomataError::MalformedPersisted {
                format_id: MATCHER_FORMAT_ID.to_string(),
                reason: err.to_string(),
            }
        })?;
        if probe.format_id != MATCHER_FORMAT_ID {
            return Err(TerraphimAutomataError::UnsupportedFormat {
                found: probe.format_id,
                expected: MATCHER_FORMAT_ID.to_string(),
            });
        }
        // Dispatch on the version tag explicitly, so that an unknown enum
        // value inside the payload (such as a bad `overlap`) is reported as
        // malformed data rather than as an unsupported version. Only a
        // string tag can name a version; any other shape falls through to
        // the full parse and is reported as malformed.
        if let Some(serde_json::Value::String(tag)) = probe.version {
            let supported = tag
                .parse::<u32>()
                .is_ok_and(|number| supported_versions().contains(&number));
            if !supported {
                return Err(TerraphimAutomataError::UnsupportedVersion {
                    format_id: MATCHER_FORMAT_ID.to_string(),
                    found: Some(tag),
                    supported: supported_versions(),
                });
            }
        }
        serde_json::from_str(json).map_err(|err| TerraphimAutomataError::MalformedPersisted {
            format_id: MATCHER_FORMAT_ID.to_string(),
            reason: err.to_string(),
        })
    }

    /// Write the envelope to a writer.
    pub fn to_writer<W: std::io::Write>(&self, writer: W) -> Result<()> {
        serde_json::to_writer_pretty(writer, self).map_err(Into::into)
    }

    /// Read an envelope from a reader, with the same validation as
    /// [`PersistedMatcherEnvelope::from_json`].
    pub fn from_reader<R: Read>(mut reader: R) -> Result<Self> {
        let mut buf = String::new();
        reader
            .read_to_string(&mut buf)
            .map_err(TerraphimAutomataError::Io)?;
        Self::from_json(&buf)
    }
}

/// Sorted list of payload versions this reader supports.
fn supported_versions() -> Vec<u32> {
    (1..=MATCHER_FORMAT_VERSION).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use terraphim_types::NormalizedTermValue;

    fn term(id: &str) -> NormalizedTerm {
        NormalizedTerm::with_auto_id(NormalizedTermValue::from(id.to_string()))
    }

    fn sample_matcher() -> Result<CompiledMatcher> {
        let options = MatcherOptions::default()
            .with_case_insensitive(false)
            .with_min_pattern_length(4);
        let mut builder = MatcherBuilder::new(options);
        builder.insert("AlphaMatch".to_string(), term("a1"))?;
        builder.insert("Beta Pattern".to_string(), term("b2"))?;
        builder.build()
    }

    #[test]
    fn round_trip_preserves_options_and_behaviour()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let matcher = sample_matcher()?;
        let envelope = PersistedMatcherEnvelope::from_matcher(&matcher);
        let json = envelope.to_json()?;

        let restored = PersistedMatcherEnvelope::from_json(&json)?.to_matcher()?;
        assert_eq!(matcher.options(), restored.options());
        assert_eq!(matcher.len(), restored.len());

        let text = "AlphaMatch then beta pattern then AlphaMatch";
        let before = matcher
            .find_matches(text, true)?
            .into_iter()
            .map(|m| (m.term, m.pos))
            .collect::<Vec<_>>();
        let after = restored
            .find_matches(text, true)?
            .into_iter()
            .map(|m| (m.term, m.pos))
            .collect::<Vec<_>>();
        let names = |ms: &[(String, Option<(usize, usize)>)]| ms.to_vec();
        let before = names(&before);
        let after = names(&after);
        assert_eq!(before, after);
        // Case-insensitivity stayed off: the lowercase 'beta pattern' does not
        // match, so only the two case-exact occurrences hit.
        assert_eq!(before.len(), 2);
        assert!(
            before
                .iter()
                .all(|(term, _)| term == "AlphaMatch" || term == "Beta Pattern")
        );
        Ok(())
    }

    #[test]
    fn envelope_carries_format_identity_and_version()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let matcher = sample_matcher()?;
        let json = PersistedMatcherEnvelope::from_matcher(&matcher).to_json()?;
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["format_id"], MATCHER_FORMAT_ID);
        assert_eq!(value["version"], "1");
        Ok(())
    }

    #[test]
    fn future_version_is_rejected_before_payload_use()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let matcher = sample_matcher()?;
        let json = PersistedMatcherEnvelope::from_matcher(&matcher).to_json()?;
        let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
        value["version"] = serde_json::Value::String("99".to_string());
        let future = serde_json::to_string(&value).unwrap();

        let err = PersistedMatcherEnvelope::from_json(&future).unwrap_err();
        match err {
            TerraphimAutomataError::UnsupportedVersion {
                format_id,
                found,
                supported,
            } => {
                assert_eq!(format_id, MATCHER_FORMAT_ID);
                assert_eq!(found.as_deref(), Some("99"));
                assert_eq!(supported, vec![1, 2]);
            }
            other => panic!("expected UnsupportedVersion, got: {other}"),
        }
        Ok(())
    }

    #[test]
    fn unknown_format_id_is_rejected() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let matcher = sample_matcher()?;
        let json = PersistedMatcherEnvelope::from_matcher(&matcher).to_json()?;
        let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
        value["format_id"] = serde_json::Value::String("someone.else.format".to_string());
        let other = serde_json::to_string(&value).unwrap();

        let err = PersistedMatcherEnvelope::from_json(&other).unwrap_err();
        assert!(matches!(
            err,
            TerraphimAutomataError::UnsupportedFormat { .. }
        ));
        Ok(())
    }

    #[test]
    fn malformed_envelope_is_rejected_with_typed_error()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let err = PersistedMatcherEnvelope::from_json("{ not json").unwrap_err();
        assert!(matches!(
            err,
            TerraphimAutomataError::MalformedPersisted { .. }
        ));

        // Missing version tag entirely.
        let matcher = sample_matcher()?;
        let json = PersistedMatcherEnvelope::from_matcher(&matcher).to_json()?;
        let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
        value.as_object_mut().unwrap().remove("version");
        let tagless = serde_json::to_string(&value).unwrap();
        let err = PersistedMatcherEnvelope::from_json(&tagless).unwrap_err();
        assert!(matches!(
            err,
            TerraphimAutomataError::UnsupportedVersion { .. }
                | TerraphimAutomataError::MalformedPersisted { .. }
        ));
        Ok(())
    }

    #[test]
    fn configuration_mismatch_is_reported_on_rebuild()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        // A persisted pattern shorter than the persisted minimum length can
        // never be rebuilt; the load must fail rather than silently adjust.
        let data = PersistedMatcherV1 {
            options: MatcherOptions::default()
                .with_case_insensitive(true)
                .with_min_pattern_length(10),
            patterns: vec![("short".to_string(), term("s"))],
        };
        let envelope = PersistedMatcherEnvelope {
            format_id: MATCHER_FORMAT_ID.to_string(),
            payload: PersistedMatcherPayload::V1(data),
        };
        let err = envelope.to_matcher().unwrap_err();
        assert!(matches!(err, TerraphimAutomataError::InvalidPattern { .. }));
        Ok(())
    }

    #[test]
    fn legacy_envelope_without_new_options_loads_with_defaults()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        // A version 1 envelope exactly as written before `overlap` and
        // `word_boundaries` existed.
        let legacy = r#"{
  "format_id": "terraphim.automata.matcher",
  "version": "1",
  "options": {
    "case_insensitive": false,
    "min_pattern_length": 4
  },
  "patterns": []
}"#;
        let envelope = PersistedMatcherEnvelope::from_json(legacy)?;
        let PersistedMatcherPayload::V1(data) = &envelope.payload else {
            return Err("legacy envelope must parse as version 1".into());
        };
        let expected = MatcherOptions::default()
            .with_case_insensitive(false)
            .with_min_pattern_length(4)
            .with_overlap(crate::compiled::MatchOverlap::LeftmostLongest)
            .with_word_boundaries(true)
            .with_allow_blank_patterns(false);
        assert_eq!(data.options, expected);

        // The same legacy shape derived from a current envelope with patterns:
        // strip the new fields and confirm the rebuilt matcher behaves as the
        // default-mode original.
        let matcher = sample_matcher()?;
        let json = PersistedMatcherEnvelope::from_matcher(&matcher).to_json()?;
        let mut value: serde_json::Value = serde_json::from_str(&json)?;
        let options = value["options"]
            .as_object_mut()
            .ok_or("options is an object")?;
        assert!(options.remove("overlap").is_some());
        assert!(options.remove("word_boundaries").is_some());
        let stripped = serde_json::to_string(&value)?;
        let restored = PersistedMatcherEnvelope::from_json(&stripped)?.to_matcher()?;
        assert_eq!(restored.options(), matcher.options());
        let text = "AlphaMatch and Beta Pattern";
        assert_eq!(
            restored.find_matches(text, true)?,
            matcher.find_matches(text, true)?
        );
        Ok(())
    }

    #[test]
    fn round_trip_preserves_overlap_and_word_boundary_options()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let options = MatcherOptions::default()
            .with_case_insensitive(false)
            .with_min_pattern_length(2)
            .with_overlap(crate::compiled::MatchOverlap::All)
            .with_word_boundaries(false);
        let mut builder = MatcherBuilder::new(options);
        builder.insert("aa".to_string(), term("aa"))?;
        builder.insert("cat".to_string(), term("cat"))?;
        let matcher = builder.build()?;

        let envelope = PersistedMatcherEnvelope::from_matcher(&matcher);
        assert_eq!(envelope.payload.version(), 2);
        let json = envelope.to_json()?;
        let value: serde_json::Value = serde_json::from_str(&json)?;
        assert_eq!(value["version"], "2");
        assert_eq!(value["options"]["overlap"], "all");
        assert_eq!(value["options"]["word_boundaries"], false);

        let parsed = PersistedMatcherEnvelope::from_json(&json)?;
        assert_eq!(parsed, envelope);
        let restored = parsed.to_matcher()?;
        assert_eq!(restored.options(), matcher.options());
        let text = "aaa concatenate";
        type Positioned = Vec<(String, Option<(usize, usize)>)>;
        let positions = |m: &CompiledMatcher| -> Result<Positioned> {
            Ok(m.find_matches(text, true)?
                .into_iter()
                .map(|found| (found.term, found.pos))
                .collect())
        };
        let before = positions(&matcher)?;
        assert_eq!(before, positions(&restored)?);
        assert_eq!(
            before,
            vec![
                ("aa".to_string(), Some((0, 2))),
                ("aa".to_string(), Some((1, 3))),
                ("cat".to_string(), Some((7, 10))),
            ]
        );
        Ok(())
    }

    /// Build a one-pattern matcher with the given options.
    fn matcher_with(options: MatcherOptions) -> Result<CompiledMatcher> {
        let mut builder = MatcherBuilder::new(options);
        builder.insert("cat".to_string(), term("cat"))?;
        builder.build()
    }

    fn written_version(options: MatcherOptions) -> Result<serde_json::Value> {
        let matcher = matcher_with(options)?;
        let json = PersistedMatcherEnvelope::from_matcher(&matcher).to_json()?;
        let value: serde_json::Value = serde_json::from_str(&json)?;
        Ok(value["version"].clone())
    }

    #[test]
    fn default_overlap_and_boundaries_write_version_1()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        assert_eq!(written_version(MatcherOptions::default())?, "1");
        // Options that older readers already understood do not force v2.
        assert_eq!(
            written_version(
                MatcherOptions::default()
                    .with_case_insensitive(false)
                    .with_min_pattern_length(3)
            )?,
            "1"
        );
        Ok(())
    }

    #[test]
    fn non_default_overlap_or_boundaries_write_version_2()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        assert_eq!(
            written_version(MatcherOptions::default().with_overlap(MatchOverlap::All))?,
            "2"
        );
        assert_eq!(
            written_version(MatcherOptions::default().with_word_boundaries(false))?,
            "2"
        );
        assert_eq!(
            written_version(
                MatcherOptions::default()
                    .with_overlap(MatchOverlap::All)
                    .with_word_boundaries(false)
            )?,
            "2"
        );
        Ok(())
    }

    #[test]
    fn version_2_round_trips_through_reader_and_writer()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let matcher = matcher_with(MatcherOptions::default().with_word_boundaries(false))?;
        let envelope = PersistedMatcherEnvelope::from_matcher(&matcher);
        let mut buf = Vec::new();
        envelope.to_writer(&mut buf)?;
        let restored_envelope = PersistedMatcherEnvelope::from_reader(&buf[..])?;
        assert_eq!(restored_envelope, envelope);
        assert_eq!(restored_envelope.payload.version(), 2);
        let restored = restored_envelope.to_matcher()?;
        assert_eq!(restored.options(), matcher.options());
        assert_eq!(
            restored.find_matches("concatenate", true)?,
            matcher.find_matches("concatenate", true)?
        );
        assert_eq!(restored.find_matches("concatenate", true)?.len(), 1);
        Ok(())
    }

    #[test]
    fn version_1_with_non_default_fields_is_honoured()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        // Pre-release builds of this change wrote non-default options under
        // version 1. Such envelopes are accepted as version 1 and the stored
        // fields are applied, not replaced by defaults.
        let early = r#"{
  "format_id": "terraphim.automata.matcher",
  "version": "1",
  "options": {
    "case_insensitive": true,
    "min_pattern_length": 2,
    "overlap": "all",
    "word_boundaries": false
  },
  "patterns": []
}"#;
        let envelope = PersistedMatcherEnvelope::from_json(early)?;
        assert_eq!(envelope.payload.version(), 1);
        assert_eq!(
            envelope.payload.options(),
            &MatcherOptions::default()
                .with_overlap(MatchOverlap::All)
                .with_word_boundaries(false)
        );
        // Re-saving upgrades it to version 2.
        let resaved = PersistedMatcherEnvelope::from_matcher(&envelope.to_matcher()?);
        assert_eq!(resaved.payload.version(), 2);
        Ok(())
    }

    #[test]
    fn version_2_requires_every_option_field() -> std::result::Result<(), Box<dyn std::error::Error>>
    {
        for missing in [
            "case_insensitive",
            "min_pattern_length",
            "overlap",
            "word_boundaries",
        ] {
            let matcher = matcher_with(MatcherOptions::default().with_overlap(MatchOverlap::All))?;
            let json = PersistedMatcherEnvelope::from_matcher(&matcher).to_json()?;
            let mut value: serde_json::Value = serde_json::from_str(&json)?;
            assert_eq!(value["version"], "2");
            let options = value["options"]
                .as_object_mut()
                .ok_or("options is an object")?;
            assert!(options.remove(missing).is_some());
            let truncated = serde_json::to_string(&value)?;
            let err = PersistedMatcherEnvelope::from_json(&truncated).unwrap_err();
            assert!(
                matches!(err, TerraphimAutomataError::MalformedPersisted { .. }),
                "missing {missing}: {err}"
            );
        }
        Ok(())
    }

    #[test]
    fn version_3_is_rejected_as_unsupported() -> std::result::Result<(), Box<dyn std::error::Error>>
    {
        let matcher = matcher_with(MatcherOptions::default().with_overlap(MatchOverlap::All))?;
        let json = PersistedMatcherEnvelope::from_matcher(&matcher).to_json()?;
        let mut value: serde_json::Value = serde_json::from_str(&json)?;
        value["version"] = serde_json::Value::String("3".to_string());
        let err = PersistedMatcherEnvelope::from_json(&serde_json::to_string(&value)?).unwrap_err();
        assert!(matches!(
            err,
            TerraphimAutomataError::UnsupportedVersion { found: Some(ref v), .. } if v == "3"
        ));
        Ok(())
    }

    #[test]
    fn bad_option_value_is_malformed_not_unsupported_version()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        for version in ["1", "2"] {
            let json = format!(
                r#"{{"format_id":"terraphim.automata.matcher","version":"{version}",
                "options":{{"case_insensitive":true,"min_pattern_length":2,
                "overlap":"bogus","word_boundaries":true}},"patterns":[]}}"#
            );
            let err = PersistedMatcherEnvelope::from_json(&json).unwrap_err();
            assert!(
                matches!(err, TerraphimAutomataError::MalformedPersisted { .. }),
                "version {version}: {err}"
            );
        }
        Ok(())
    }

    #[test]
    fn non_string_version_tag_is_malformed() -> std::result::Result<(), Box<dyn std::error::Error>>
    {
        let json = r#"{"format_id":"terraphim.automata.matcher","version":1,
            "options":{"case_insensitive":true,"min_pattern_length":2},"patterns":[]}"#;
        let err = PersistedMatcherEnvelope::from_json(json).unwrap_err();
        assert!(
            matches!(err, TerraphimAutomataError::MalformedPersisted { .. }),
            "{err}"
        );
        Ok(())
    }

    /// A matcher with a whitespace-only pattern (Refs #86).
    fn blank_pattern_matcher() -> Result<CompiledMatcher> {
        let options = MatcherOptions::default()
            .with_allow_blank_patterns(true)
            .with_overlap(MatchOverlap::All)
            .with_word_boundaries(false);
        let mut builder = MatcherBuilder::new(options);
        builder.insert("  ".to_string(), term("gap"))?;
        builder.insert("cat".to_string(), term("cat"))?;
        builder.build()
    }

    #[test]
    fn allow_blank_patterns_writes_version_2_and_round_trips()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        assert_eq!(
            written_version(MatcherOptions::default().with_allow_blank_patterns(true))?,
            "2"
        );
        let matcher = blank_pattern_matcher()?;
        let envelope = PersistedMatcherEnvelope::from_matcher(&matcher);
        assert_eq!(envelope.payload.version(), 2);
        let json = envelope.to_json()?;
        let value: serde_json::Value = serde_json::from_str(&json)?;
        assert_eq!(value["options"]["allow_blank_patterns"], true);

        let parsed = PersistedMatcherEnvelope::from_json(&json)?;
        assert_eq!(parsed, envelope);
        let restored = parsed.to_matcher()?;
        assert_eq!(restored.options(), matcher.options());
        let text = "a    cat";
        assert_eq!(
            restored.find_matches(text, true)?,
            matcher.find_matches(text, true)?
        );
        assert_eq!(restored.find_matches(text, true)?.len(), 4);
        Ok(())
    }

    #[test]
    fn default_envelope_stays_version_1_with_allow_blank_patterns_false()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let json = PersistedMatcherEnvelope::from_matcher(&sample_matcher()?).to_json()?;
        let value: serde_json::Value = serde_json::from_str(&json)?;
        assert_eq!(value["version"], "1");
        assert_eq!(value["options"]["allow_blank_patterns"], false);
        Ok(())
    }

    #[test]
    fn version_2_envelope_from_2_0_0_loads_without_allow_blank_patterns()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        // Exactly as 2.0.0 writes a non-default matcher: every option field
        // of that release, no `allow_blank_patterns`.
        let legacy = r#"{
  "format_id": "terraphim.automata.matcher",
  "version": "2",
  "options": {
    "case_insensitive": true,
    "min_pattern_length": 2,
    "overlap": "all",
    "word_boundaries": false
  },
  "patterns": [["cat", {"id": 1, "nterm": "cat"}]]
}"#;
        let envelope = PersistedMatcherEnvelope::from_json(legacy)?;
        assert_eq!(envelope.payload.version(), 2);
        assert_eq!(
            envelope.payload.options(),
            &MatcherOptions::default()
                .with_overlap(MatchOverlap::All)
                .with_word_boundaries(false)
        );
        let matcher = envelope.to_matcher()?;
        assert_eq!(matcher.find_matches("concatenate", true)?.len(), 1);
        Ok(())
    }

    #[test]
    fn version_1_with_allow_blank_patterns_is_honoured()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let early = r#"{
  "format_id": "terraphim.automata.matcher",
  "version": "1",
  "options": {
    "case_insensitive": true,
    "min_pattern_length": 2,
    "allow_blank_patterns": true
  },
  "patterns": [["  ", {"id": 1, "nterm": "gap"}]]
}"#;
        let envelope = PersistedMatcherEnvelope::from_json(early)?;
        assert!(envelope.payload.options().allow_blank_patterns);
        let matcher = envelope.to_matcher()?;
        assert_eq!(matcher.find_matches("-  -", true)?.len(), 1);
        // Re-saving upgrades it to version 2.
        assert_eq!(
            PersistedMatcherEnvelope::from_matcher(&matcher)
                .payload
                .version(),
            2
        );
        Ok(())
    }

    #[test]
    fn reader_and_writer_round_trip() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let matcher = sample_matcher()?;
        let envelope = PersistedMatcherEnvelope::from_matcher(&matcher);
        let mut buf = Vec::new();
        envelope.to_writer(&mut buf).unwrap();
        let restored = PersistedMatcherEnvelope::from_reader(&buf[..]).unwrap();
        assert_eq!(envelope, restored);
        Ok(())
    }
}
