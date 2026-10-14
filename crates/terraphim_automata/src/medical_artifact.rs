//! Artifact I/O for ShardedUmlsExtractor
//!
//! Custom format because daachorse shards use raw bytes (not bincode-compatible),
//! while the metadata is bincode-serialized alongside the shard data.
//!
//! Versioned envelope format (v2, current):
//!   [magic: "UMLS" (4 bytes)]
//!   [version: u16 LE (= 2)]
//!   [header_len: u64 LE]
//!   [header_bytes: bincode(ArtifactHeader)]
//!   for each shard in header.shard_byte_lengths:
//!     [shard_bytes: daachorse-serialised shard bytes (opaque payload)]
//!
//! Integrity and trust model:
//! - Every loaded artefact is treated as untrusted input, regardless of
//!   transport source or checksum validity. A checksum establishes accidental
//!   corruption detection only; it is NOT a signature and proves nothing about
//!   provenance.
//! - For this reason the loader never hands shard bytes to unchecked
//!   deserialisation. Callers rebuild matchers from the validated pattern
//!   metadata carried in the header (see `ShardedUmlsExtractor`).
//! - Legacy (v1) artefacts -- which lack the magic/version prefix -- are
//!   rejected with an actionable error; migrate by rebuilding from source
//!   TSV (`UmlsDataset::from_tsv` + `ShardedUmlsExtractor::from_dataset` +
//!   `save_to_artifact`).
//! - Decompression is bounded: expanded size, expansion ratio, compressed
//!   size, shard count, per-shard size and aggregate size are all checked
//!   with integer-safe arithmetic before or during allocation.
//! - All failures are typed (`ArtifactError`), deterministic and fail closed;
//!   no partially loaded state escapes on error.

use sha2::{Digest, Sha256};

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;

use crate::umls::UmlsConcept;

/// Magic bytes prefixing every v2 artefact.
pub const ARTIFACT_MAGIC: [u8; 4] = *b"UMLS";
/// Current artefact format version.
pub const ARTIFACT_VERSION: u16 = 2;

/// Resource budgets applied while loading an artefact.
///
/// Limits are explicit, have secure defaults, and may be lowered by callers.
/// There is no way to disable a limit: setting one to zero makes the loader
/// fail closed (reject everything), never load unbounded.
#[derive(Debug, Clone)]
pub struct ArtifactLimits {
    /// Maximum accepted compressed artefact size in bytes.
    pub max_compressed_bytes: usize,
    /// Maximum decompressed payload size in bytes.
    pub max_expanded_bytes: usize,
    /// Maximum allowed expansion ratio (expanded / compressed).
    pub max_expansion_ratio: u64,
    /// Maximum serialised header size in bytes.
    pub max_header_bytes: usize,
    /// Maximum number of shards.
    pub max_shard_count: usize,
    /// Maximum aggregate raw shard bytes.
    pub max_total_shard_bytes: usize,
    /// Maximum number of patterns across all shards.
    pub max_patterns: usize,
    /// Maximum length of a single pattern (term) in bytes.
    pub max_pattern_len: usize,
    /// Maximum total bytes across all decoded patterns.
    pub max_total_pattern_bytes: usize,
}

impl Default for ArtifactLimits {
    fn default() -> Self {
        Self {
            max_compressed_bytes: 1 << 30, // 1 GiB
            max_expanded_bytes: 4 << 30,   // 4 GiB
            max_expansion_ratio: 1024,
            max_header_bytes: 64 << 20, // 64 MiB
            max_shard_count: 4096,
            max_total_shard_bytes: 4 << 30, // 4 GiB
            max_patterns: 8_000_000,
            max_pattern_len: 4096,
            max_total_pattern_bytes: 512 << 20, // 512 MiB
        }
    }
}

/// Typed errors for artefact loading and saving.
#[derive(Debug, thiserror::Error)]
pub enum ArtifactError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error(
        "compressed artefact is {actual} bytes, exceeding the configured limit of {limit} bytes"
    )]
    CompressedTooLarge { actual: usize, limit: usize },

    #[error(
        "decompressed artefact is {actual} bytes, exceeding the configured limit of {limit} bytes"
    )]
    ExpandedTooLarge { actual: usize, limit: usize },

    #[error(
        "decompression ratio {ratio}:1 exceeds the configured limit of {limit}:1 (compressed {compressed} bytes, expanded {expanded} bytes)"
    )]
    ExpansionRatioTooLarge {
        ratio: u64,
        limit: u64,
        compressed: usize,
        expanded: usize,
    },

    #[error("header is {actual} bytes, exceeding the configured limit of {limit} bytes")]
    HeaderTooLarge { actual: usize, limit: usize },

    #[error("artefact declares {actual} shards, exceeding the configured limit of {limit}")]
    TooManyShards { actual: usize, limit: usize },

    #[error("shard {index} is {actual} bytes, exceeding the configured limit of {limit} bytes")]
    ShardTooLarge {
        index: usize,
        actual: usize,
        limit: usize,
    },

    #[error("aggregate shard bytes {actual} exceed the configured limit of {limit} bytes")]
    AggregateTooLarge { actual: usize, limit: usize },

    #[error("artefact truncated: {context}")]
    Truncated { context: &'static str },

    #[error("{extra} trailing bytes after the final shard; artefact is malformed")]
    TrailingBytes { extra: u64 },

    #[error(
        "unsupported artefact version {found} (this build supports version {supported}); re-save the artefact with a compatible release"
    )]
    UnsupportedVersion { found: u16, supported: u16 },

    #[error(
        "legacy artefact without version envelope detected; rebuild it from the source TSV with UmlsDataset::from_tsv, ShardedUmlsExtractor::from_dataset and save_to_artifact"
    )]
    LegacyFormat,

    #[error("artefact header is corrupt or malformed: {0}")]
    CorruptHeader(String),

    #[error("shard {shard} checksum mismatch: artefact may be corrupt or tampered with")]
    Integrity { shard: usize },

    #[error("artefact metadata failed validation: {0}")]
    InvalidMetadata(String),

    #[error(
        "shard {shard} patterns are not strictly ascending and unique as required for matcher construction"
    )]
    PatternsUnsorted { shard: usize },

    #[error("artefact declares {actual} patterns, exceeding the configured limit of {limit}")]
    TooManyPatterns { actual: usize, limit: usize },

    #[error("pattern {index} is {actual} bytes, exceeding the configured limit of {limit} bytes")]
    PatternTooLong {
        index: usize,
        actual: usize,
        limit: usize,
    },

    #[error("failed to rebuild shard {shard} automaton from validated patterns: {detail}")]
    AutomatonBuild { shard: usize, detail: String },
}

/// Serializable pattern metadata
///
/// A single term may map to multiple CUIs (e.g., "cold" maps to both
/// C0009264 "Common Cold" and C0234192 "Cold Temperature"). All CUIs
/// are preserved to avoid silent data loss during deduplication.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PatternMeta {
    pub cuis: Vec<String>,
    pub term: String,
}

/// Artifact header: everything that can be bincode-serialized
#[derive(Debug, Serialize, Deserialize)]
pub struct ArtifactHeader {
    /// Per-shard pattern metadata. Within each shard, terms must be
    /// strictly ascending and unique (sorted, no duplicates).
    pub shard_metadata: Vec<Vec<PatternMeta>>,
    /// Concept lookup by CUI
    pub concept_index: HashMap<String, UmlsConcept>,
    /// Total patterns across all shards
    pub total_patterns: usize,
    /// Raw byte length of each daachorse shard (order matches shard_metadata)
    pub shard_byte_lengths: Vec<usize>,
    /// SHA-256 digest of each shard's raw bytes.
    pub shard_checksums: Vec<[u8; 32]>,
}

/// Validate header metadata against limits with integer-safe arithmetic.
///
/// Checks shard-count, pattern-count, pattern-length, aggregate-shard-bytes
/// and aggregate-pattern-bytes budgets, plus structural invariants needed for
/// safe matcher reconstruction (consistent counts, sorted unique terms).
fn validate_header(header: &ArtifactHeader, limits: &ArtifactLimits) -> Result<(), ArtifactError> {
    let shard_count = header.shard_byte_lengths.len();
    if shard_count > limits.max_shard_count {
        return Err(ArtifactError::TooManyShards {
            actual: shard_count,
            limit: limits.max_shard_count,
        });
    }
    if header.shard_metadata.len() != shard_count || header.shard_checksums.len() != shard_count {
        return Err(ArtifactError::InvalidMetadata(
            "shard_metadata, shard_byte_lengths and shard_checksums counts differ".to_string(),
        ));
    }
    if shard_count == 0 {
        return Err(ArtifactError::InvalidMetadata(
            "artefact declares zero shards".to_string(),
        ));
    }

    // Aggregate shard bytes, checked.
    let mut aggregate: usize = 0;
    for (i, &len) in header.shard_byte_lengths.iter().enumerate() {
        if len > limits.max_total_shard_bytes {
            return Err(ArtifactError::ShardTooLarge {
                index: i,
                actual: len,
                limit: limits.max_total_shard_bytes,
            });
        }
        aggregate = aggregate
            .checked_add(len)
            .ok_or(ArtifactError::AggregateTooLarge {
                actual: usize::MAX,
                limit: limits.max_total_shard_bytes,
            })?;
        if aggregate > limits.max_total_shard_bytes {
            return Err(ArtifactError::AggregateTooLarge {
                actual: aggregate,
                limit: limits.max_total_shard_bytes,
            });
        }
    }

    // Pattern budgets and per-shard structural invariants.
    let mut pattern_count: usize = 0;
    let mut total_pattern_bytes: usize = 0;
    for (shard_idx, shard) in header.shard_metadata.iter().enumerate() {
        if shard.is_empty() {
            return Err(ArtifactError::InvalidMetadata(format!(
                "shard {} declares no patterns",
                shard_idx
            )));
        }
        let mut prev: Option<&str> = None;
        for (pattern_idx, meta) in shard.iter().enumerate() {
            if meta.term.is_empty() {
                return Err(ArtifactError::InvalidMetadata(format!(
                    "shard {} pattern {} is empty",
                    shard_idx, pattern_idx
                )));
            }
            let term_len = meta.term.len();
            if term_len > limits.max_pattern_len {
                return Err(ArtifactError::PatternTooLong {
                    index: pattern_count + pattern_idx,
                    actual: term_len,
                    limit: limits.max_pattern_len,
                });
            }
            total_pattern_bytes =
                total_pattern_bytes
                    .checked_add(term_len)
                    .ok_or(ArtifactError::InvalidMetadata(
                        "total pattern byte count overflowed".to_string(),
                    ))?;
            if total_pattern_bytes > limits.max_total_pattern_bytes {
                return Err(ArtifactError::InvalidMetadata(format!(
                    "total pattern bytes {} exceed the configured limit of {} bytes",
                    total_pattern_bytes, limits.max_total_pattern_bytes
                )));
            }
            if let Some(p) = prev {
                if p >= meta.term.as_str() {
                    return Err(ArtifactError::PatternsUnsorted { shard: shard_idx });
                }
            }
            prev = Some(meta.term.as_str());
        }
        pattern_count =
            pattern_count
                .checked_add(shard.len())
                .ok_or(ArtifactError::TooManyPatterns {
                    actual: usize::MAX,
                    limit: limits.max_patterns,
                })?;
        if pattern_count > limits.max_patterns {
            return Err(ArtifactError::TooManyPatterns {
                actual: pattern_count,
                limit: limits.max_patterns,
            });
        }
    }

    if header.total_patterns != pattern_count {
        return Err(ArtifactError::InvalidMetadata(format!(
            "header total_patterns is {} but shard metadata holds {} patterns",
            header.total_patterns, pattern_count
        )));
    }
    Ok(())
}

/// Decompress `compressed` under the configured budgets.
///
/// Decoding is streamed and bounded: if the expanded size would exceed
/// `max_expanded_bytes`, the ratio would exceed `max_expansion_ratio`, or the
/// compressed size exceeds `max_compressed_bytes`, a typed error is returned
/// and no oversized allocation is performed.
fn decode_bounded(compressed: &[u8], limits: &ArtifactLimits) -> Result<Vec<u8>, ArtifactError> {
    if compressed.len() > limits.max_compressed_bytes {
        return Err(ArtifactError::CompressedTooLarge {
            actual: compressed.len(),
            limit: limits.max_compressed_bytes,
        });
    }

    let budget = limits.max_expanded_bytes.saturating_add(1);
    let mut reader = zstd::stream::Decoder::new(compressed)?.take(budget as u64);
    let mut raw = Vec::new();
    reader.read_to_end(&mut raw)?;

    if raw.len() > limits.max_expanded_bytes {
        return Err(ArtifactError::ExpandedTooLarge {
            actual: raw.len(),
            limit: limits.max_expanded_bytes,
        });
    }
    // Ratio check with u128 arithmetic: a small compressed payload that
    // expands hugely is rejected even if the absolute size fits the budget.
    let expanded = raw.len() as u128;
    let compressed_len = compressed.len() as u128;
    if compressed_len > 0 {
        let scaled = compressed_len
            .checked_mul(limits.max_expansion_ratio as u128)
            .expect("ratio multiplication cannot overflow u128 for realistic limits");
        if expanded > scaled {
            // Effective ratio, rounded up, purely for the error message.
            let ratio = expanded.div_ceil(compressed_len) as u64;
            return Err(ArtifactError::ExpansionRatioTooLarge {
                ratio,
                limit: limits.max_expansion_ratio,
                compressed: compressed.len(),
                expanded: raw.len(),
            });
        }
    }
    Ok(raw)
}

/// Save a UMLS artifact: header (bincode) + shard bytes, compressed with zstd,
/// wrapped in the versioned envelope.
///
/// Computes SHA-256 of each shard and stores digests in the header.
pub fn save_umls_artifact(
    header: &ArtifactHeader,
    shard_bytes: &[Vec<u8>],
    path: &Path,
) -> Result<(), ArtifactError> {
    if header.shard_byte_lengths.len() != shard_bytes.len() {
        return Err(ArtifactError::InvalidMetadata(
            "shard_byte_lengths must match shard_bytes count".to_string(),
        ));
    }
    if header.shard_checksums.len() != shard_bytes.len() {
        return Err(ArtifactError::InvalidMetadata(
            "shard_checksums must match shard_bytes count".to_string(),
        ));
    }

    // Encode header with bincode
    let header_encoded = bincode::serialize(header)
        .map_err(|e| ArtifactError::CorruptHeader(format!("failed to serialise header: {}", e)))?;
    let header_len = header_encoded.len() as u64;

    // Build raw payload: [magic][version][header_len (8 bytes)][header][shards]
    let mut raw = Vec::new();
    raw.write_all(&ARTIFACT_MAGIC)?;
    raw.write_all(&ARTIFACT_VERSION.to_le_bytes())?;
    raw.write_all(&header_len.to_le_bytes())?;
    raw.write_all(&header_encoded)?;
    for shard in shard_bytes {
        raw.write_all(shard)?;
    }

    // Compress with zstd (level 3 is a good balance of speed vs ratio)
    let compressed = zstd::encode_all(&raw[..], 3)?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, &compressed)?;

    // Artefacts contain data used for matcher construction; write them
    // owner-only so the permission gate in `load_umls_artifact_with_limits`
    // accepts freshly saved files and other users cannot tamper with them.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }

    log::info!(
        "Saved UMLS artifact to {:?}: {} shards, {} total patterns, {} bytes compressed ({} bytes raw)",
        path,
        shard_bytes.len(),
        header.total_patterns,
        compressed.len(),
        raw.len()
    );
    Ok(())
}

/// Load a UMLS artifact using the default [`ArtifactLimits`].
pub fn load_umls_artifact(path: &Path) -> Result<(ArtifactHeader, Vec<Vec<u8>>), ArtifactError> {
    load_umls_artifact_with_limits(path, &ArtifactLimits::default())
}

/// Load a UMLS artifact under explicit resource budgets.
///
/// Returns `(header, shard_bytes)` only after version, size, integrity and
/// metadata validation all pass. Shard bytes are opaque payloads: callers
/// must rebuild matchers from the validated pattern metadata, never
/// deserialise them directly.
pub fn load_umls_artifact_with_limits(
    path: &Path,
    limits: &ArtifactLimits,
) -> Result<(ArtifactHeader, Vec<Vec<u8>>), ArtifactError> {
    // Permission enforcement: any write bit beyond the owner is a hard
    // error.  An attacker who can write the artifact can forge both bytes
    // and stored checksums together, bypassing the integrity gate.  The
    // checksum guard is only effective when the file is owner-writable
    // exclusively.  Binding this to Unix keeps Windows builds clean;
    // Windows callers should apply ACLs externally.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(path) {
            let mode = meta.permissions().mode();
            let mut reasons = Vec::new();
            if mode & 0o002 != 0 {
                reasons.push("world-writable".to_string());
            }
            if mode & 0o020 != 0 {
                reasons.push("group-writable".to_string());
            }
            if !reasons.is_empty() {
                return Err(ArtifactError::Io(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    format!(
                        "Artifact {:?} has insecure permissions (mode {:04o}): {}. \
                         Restrict to owner-only write (e.g. chmod 600) and re-save.",
                        path,
                        mode,
                        reasons.join(", ")
                    ),
                )));
            }
        }
    }

    let compressed = std::fs::read(path)?;
    let raw = decode_bounded(&compressed, limits)?;

    if raw.len() < 6 {
        return Err(ArtifactError::Truncated {
            context: "version envelope",
        });
    }

    let (raw, header_offset) = if raw[..4] == ARTIFACT_MAGIC {
        let version = u16::from_le_bytes(raw[4..6].try_into().expect("6 >= 6"));
        if version != ARTIFACT_VERSION {
            return Err(ArtifactError::UnsupportedVersion {
                found: version,
                supported: ARTIFACT_VERSION,
            });
        }
        if raw.len() < 14 {
            return Err(ArtifactError::Truncated {
                context: "header length prefix",
            });
        }
        (raw, 6usize)
    } else {
        return Err(ArtifactError::LegacyFormat);
    };

    if raw.len() < header_offset + 8 {
        return Err(ArtifactError::Truncated {
            context: "header length prefix",
        });
    }
    let header_len = u64::from_le_bytes(raw[header_offset..header_offset + 8].try_into().unwrap());
    if header_len > limits.max_header_bytes as u64 {
        return Err(ArtifactError::HeaderTooLarge {
            actual: header_len as usize,
            limit: limits.max_header_bytes,
        });
    }
    let header_start = header_offset + 8;
    let header_end = header_start
        .checked_add(header_len as usize)
        .ok_or(ArtifactError::Truncated { context: "header" })?;
    if raw.len() < header_end {
        return Err(ArtifactError::Truncated { context: "header" });
    }

    // Deserialize header
    let header: ArtifactHeader = bincode::deserialize(&raw[header_start..header_end])
        .map_err(|e| ArtifactError::CorruptHeader(format!("header failed to decode: {}", e)))?;

    // Structural and budget validation before any shard allocation.
    validate_header(&header, limits)?;

    // Read each shard's raw bytes and verify SHA-256 integrity. The bytes
    // remain opaque: reconstruction goes through validated patterns, never
    // unchecked deserialisation.
    let mut offset = header_end;
    let mut shard_bytes = Vec::with_capacity(header.shard_byte_lengths.len());
    for (i, (&shard_len, expected_checksum)) in header
        .shard_byte_lengths
        .iter()
        .zip(header.shard_checksums.iter())
        .enumerate()
    {
        let shard_end = offset
            .checked_add(shard_len)
            .ok_or(ArtifactError::Truncated {
                context: "shard region",
            })?;
        if shard_end > raw.len() {
            return Err(ArtifactError::Truncated {
                context: "shard region",
            });
        }
        let shard_slice = &raw[offset..shard_end];
        let actual_checksum: [u8; 32] = Sha256::digest(shard_slice).into();
        if &actual_checksum != expected_checksum {
            return Err(ArtifactError::Integrity { shard: i });
        }
        shard_bytes.push(shard_slice.to_vec());
        offset = shard_end;
    }

    if offset < raw.len() {
        return Err(ArtifactError::TrailingBytes {
            extra: (raw.len() - offset) as u64,
        });
    }

    log::info!(
        "Loaded UMLS artifact from {:?}: {} shards, {} total patterns, {} bytes compressed",
        path,
        shard_bytes.len(),
        header.total_patterns,
        compressed.len()
    );
    Ok((header, shard_bytes))
}

/// Check if an artifact file exists
pub fn artifact_exists(path: &Path) -> bool {
    path.exists() && path.is_file()
}
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_test_header(shard_bytes: &[Vec<u8>]) -> ArtifactHeader {
        ArtifactHeader {
            shard_metadata: vec![
                vec![
                    PatternMeta {
                        cuis: vec!["C0000001".to_string()],
                        term: "lung cancer".to_string(),
                    },
                    PatternMeta {
                        cuis: vec!["C0000001".to_string()],
                        term: "nsclc".to_string(),
                    },
                ],
                vec![PatternMeta {
                    cuis: vec!["C0000002".to_string()],
                    term: "egfr".to_string(),
                }],
            ],
            concept_index: {
                let mut m = HashMap::new();
                m.insert(
                    "C0000001".to_string(),
                    UmlsConcept {
                        cui: "C0000001".to_string(),
                        terms: vec!["lung cancer".to_string(), "nsclc".to_string()],
                        preferred_term: "lung cancer".to_string(),
                    },
                );
                m
            },
            total_patterns: 3,
            shard_byte_lengths: shard_bytes.iter().map(|b| b.len()).collect(),
            shard_checksums: shard_bytes
                .iter()
                .map(|b| Sha256::digest(b).into())
                .collect(),
        }
    }

    #[test]
    fn test_artifact_round_trip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("umls.bin.zst");

        let shard_bytes = vec![vec![1u8; 10], vec![2u8; 8]];
        let header = make_test_header(&shard_bytes);

        save_umls_artifact(&header, &shard_bytes, &path).unwrap();
        assert!(path.exists());

        let (loaded_header, loaded_shards) = load_umls_artifact(&path).unwrap();
        assert_eq!(loaded_header.total_patterns, 3);
        assert_eq!(loaded_header.shard_metadata.len(), 2);
        assert_eq!(loaded_header.shard_byte_lengths, vec![10, 8]);
        assert_eq!(loaded_shards[0], vec![1u8; 10]);
        assert_eq!(loaded_shards[1], vec![2u8; 8]);
        assert!(loaded_header.concept_index.contains_key("C0000001"));
    }

    #[test]
    fn test_artifact_checksum_mismatch_rejected() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("tampered.bin.zst");

        let shard_bytes = vec![vec![1u8; 10], vec![2u8; 8]];
        let header = make_test_header(&shard_bytes);
        save_umls_artifact(&header, &shard_bytes, &path).unwrap();

        // Load, tamper with shard bytes in the decompressed payload, recompress
        let compressed = std::fs::read(&path).unwrap();
        let mut raw = zstd::decode_all(&compressed[..]).unwrap();
        // Flip one byte in the first shard (after the envelope and header)
        let header_len = u64::from_le_bytes(raw[6..14].try_into().unwrap()) as usize;
        raw[14 + header_len] ^= 0xFF;
        let recompressed = zstd::encode_all(&raw[..], 3).unwrap();
        std::fs::write(&path, recompressed).unwrap();

        let result = load_umls_artifact(&path);
        assert!(result.is_err(), "tampered artifact must be rejected");
        let msg = result.err().unwrap().to_string();
        assert!(msg.contains("checksum mismatch"), "error: {}", msg);
    }

    /// Verify that a world-writable artifact is rejected as a hard error.
    ///
    /// An attacker who can write the file can forge both bytes and
    /// checksums.  The permission gate rejects before any data is read.
    #[test]
    #[cfg(unix)]
    fn test_world_writable_artifact_rejected() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        let path = dir.path().join("world_writable.bin.zst");

        let shard_bytes = vec![vec![1u8; 10], vec![2u8; 8]];
        let header = make_test_header(&shard_bytes);
        save_umls_artifact(&header, &shard_bytes, &path).unwrap();

        // Make world-writable (0o666)
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o666);
        std::fs::set_permissions(&path, perms).unwrap();

        let result = load_umls_artifact(&path);
        assert!(result.is_err(), "World-writable artifact must be rejected");
        let msg = result.err().unwrap().to_string();
        assert!(
            msg.contains("world-writable"),
            "error must mention world-writable, got: {msg}"
        );
    }

    /// Verify that a group-writable artifact is rejected.
    #[test]
    #[cfg(unix)]
    fn test_group_writable_artifact_rejected() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        let path = dir.path().join("group_writable.bin.zst");

        let shard_bytes = vec![vec![1u8; 10], vec![2u8; 8]];
        let header = make_test_header(&shard_bytes);
        save_umls_artifact(&header, &shard_bytes, &path).unwrap();

        // Make group-writable (0o620)
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o620);
        std::fs::set_permissions(&path, perms).unwrap();

        let result = load_umls_artifact(&path);
        assert!(result.is_err(), "Group-writable artifact must be rejected");
        let msg = result.err().unwrap().to_string();
        assert!(
            msg.contains("group-writable"),
            "error must mention group-writable, got: {msg}"
        );
    }

    /// Verify that an artifact with secure owner-only permissions (0o600)
    /// loads successfully.
    #[test]
    #[cfg(unix)]
    fn test_secure_permissions_artifact_loads_ok() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        let path = dir.path().join("secure.bin.zst");

        let shard_bytes = vec![vec![1u8; 10], vec![2u8; 8]];
        let header = make_test_header(&shard_bytes);
        save_umls_artifact(&header, &shard_bytes, &path).unwrap();

        // Secure: owner read+write only
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o600);
        std::fs::set_permissions(&path, perms).unwrap();

        let result = load_umls_artifact(&path);
        assert!(
            result.is_ok(),
            "Owner-writable-only artifact must load; got: {:?}",
            result.err()
        );
        let (loaded_header, loaded_shards) = result.unwrap();
        assert_eq!(loaded_header.total_patterns, 3);
        assert_eq!(loaded_shards.len(), 2);
    }

    #[test]
    fn test_artifact_exists() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.bin.zst");
        assert!(!artifact_exists(&path));

        let shard_bytes = vec![vec![0u8; 10], vec![0u8; 8]];
        let header = make_test_header(&shard_bytes);
        save_umls_artifact(&header, &shard_bytes, &path).unwrap();
        assert!(artifact_exists(&path));
    }
    // ---------- Version envelope tests ----------

    fn make_v2_header(shard_bytes: &[Vec<u8>]) -> ArtifactHeader {
        // One shard_metadata Vec per shard, each with at least one pattern.
        // Header's pattern list length must equal shard count (validate_header invariant).
        let shard_metadata: Vec<Vec<PatternMeta>> = shard_bytes
            .iter()
            .enumerate()
            .map(|(i, _)| {
                let alpha = format!("alpha{i}");
                let beta = format!("beta{i}");
                vec![
                    PatternMeta {
                        cuis: vec![format!("C{:07}", i * 2 + 1)],
                        term: alpha,
                    },
                    PatternMeta {
                        cuis: vec![format!("C{:07}", i * 2 + 2)],
                        term: beta,
                    },
                ]
            })
            .collect();
        ArtifactHeader {
            shard_metadata,
            concept_index: HashMap::new(),
            total_patterns: shard_bytes.len() * 2,
            shard_byte_lengths: shard_bytes.iter().map(|b| b.len()).collect(),
            shard_checksums: shard_bytes
                .iter()
                .map(|b| Sha256::digest(b).into())
                .collect(),
        }
    }

    /// Build raw v2 payload bytes (uncompressed, caller compresses).
    fn build_v2_raw(header: &ArtifactHeader, shards: &[Vec<u8>]) -> Vec<u8> {
        let header_encoded = bincode::serialize(header).unwrap();
        let mut raw = Vec::new();
        raw.extend_from_slice(&ARTIFACT_MAGIC);
        raw.extend_from_slice(&ARTIFACT_VERSION.to_le_bytes());
        raw.extend_from_slice(&(header_encoded.len() as u64).to_le_bytes());
        raw.extend_from_slice(&header_encoded);
        for s in shards {
            raw.extend_from_slice(s);
        }
        raw
    }

    fn write_artifact(raw: &[u8], path: &Path) {
        let compressed = zstd::encode_all(raw, 3).unwrap();
        std::fs::write(path, compressed).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
    }

    #[test]
    fn test_v2_envelope_round_trip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("v2.bin.zst");
        let shards = vec![vec![7u8; 16]];
        let header = make_v2_header(&shards);
        save_umls_artifact(&header, &shards, &path).unwrap();

        let compressed = std::fs::read(&path).unwrap();
        let raw = zstd::decode_all(&compressed[..]).unwrap();
        assert_eq!(&raw[..4], &ARTIFACT_MAGIC, "magic must prefix artefacts");
        assert_eq!(
            u16::from_le_bytes(raw[4..6].try_into().unwrap()),
            ARTIFACT_VERSION
        );

        let (loaded, loaded_shards) = load_umls_artifact(&path).unwrap();
        assert_eq!(loaded.total_patterns, 2);
        assert_eq!(loaded_shards, shards);
    }

    #[test]
    fn test_unsupported_version_rejected_before_header_decode() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("future.bin.zst");
        let shards = vec![vec![1u8; 8]];
        let header = make_v2_header(&shards);
        let mut raw = build_v2_raw(&header, &shards);
        raw[4] = 99; // unknown future version
        raw[5] = 0;
        // Corrupt the header too: rejection must happen before any decode.
        for b in raw[14..].iter_mut() {
            *b = 0xFF;
        }
        write_artifact(&raw, &path);

        let err = load_umls_artifact(&path).unwrap_err();
        assert!(
            matches!(err, ArtifactError::UnsupportedVersion { found: 99, .. }),
            "expected UnsupportedVersion, got: {err:?}"
        );
    }

    #[test]
    fn test_legacy_artifact_rejected_with_actionable_error() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("legacy.bin.zst");
        // Legacy v1 shape: [header_len u64][header][shards], no magic/version.
        let shards = vec![vec![1u8; 8]];
        let header = make_v2_header(&shards);
        let header_encoded = bincode::serialize(&header).unwrap();
        let mut raw = Vec::new();
        raw.extend_from_slice(&(header_encoded.len() as u64).to_le_bytes());
        raw.extend_from_slice(&header_encoded);
        raw.extend_from_slice(&shards[0]);
        write_artifact(&raw, &path);

        let err = load_umls_artifact(&path).unwrap_err();
        let msg = err.to_string();
        assert!(
            matches!(err, ArtifactError::LegacyFormat),
            "expected LegacyFormat, got: {err:?}"
        );
        assert!(
            msg.contains("rebuild"),
            "legacy error must explain the migration path: {msg}"
        );
    }

    // ---------- Resource limit tests ----------

    #[test]
    fn test_compressed_size_limit_enforced() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("big.bin.zst");
        let shards = vec![vec![1u8; 64]];
        let header = make_v2_header(&shards);
        save_umls_artifact(&header, &shards, &path).unwrap();

        let compressed_len = std::fs::metadata(&path).unwrap().len() as usize;
        let mut limits = ArtifactLimits {
            max_compressed_bytes: compressed_len, // exactly at the boundary: allowed
            ..ArtifactLimits::default()
        };
        assert!(load_umls_artifact_with_limits(&path, &limits).is_ok());

        if compressed_len > 0 {
            limits.max_compressed_bytes = compressed_len - 1;
            let err = load_umls_artifact_with_limits(&path, &limits).unwrap_err();
            assert!(
                matches!(err, ArtifactError::CompressedTooLarge { .. }),
                "expected CompressedTooLarge, got: {err:?}"
            );
        }
    }

    #[test]
    fn test_expanded_size_limit_enforced() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("expanded.bin.zst");
        let shards = vec![vec![1u8; 4096]];
        let header = make_v2_header(&shards);
        save_umls_artifact(&header, &shards, &path).unwrap();

        // Determine the raw payload size by decompressing once.
        let raw = zstd::decode_all(&std::fs::read(&path).unwrap()[..]).unwrap();

        let mut limits = ArtifactLimits {
            max_expanded_bytes: raw.len(), // boundary: allowed
            ..ArtifactLimits::default()
        };
        assert!(load_umls_artifact_with_limits(&path, &limits).is_ok());

        if !raw.is_empty() {
            limits.max_expanded_bytes = raw.len() - 1;
            let err = load_umls_artifact_with_limits(&path, &limits).unwrap_err();
            assert!(
                matches!(err, ArtifactError::ExpandedTooLarge { .. }),
                "expected ExpandedTooLarge, got: {err:?}"
            );
        }
    }

    #[test]
    fn test_high_expansion_ratio_rejected() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("ratio.bin.zst");
        // Highly repetitive payload compresses extremely well.
        let shards = vec![vec![0u8; 1 << 20]];
        let header = make_v2_header(&shards);
        save_umls_artifact(&header, &shards, &path).unwrap();

        let compressed_len = std::fs::metadata(&path).unwrap().len() as usize;
        let raw = zstd::decode_all(&std::fs::read(&path).unwrap()[..]).unwrap();
        let natural_ratio = raw.len() as u128 / compressed_len.max(1) as u128;
        // Absolute budget generous (default), ratio budget tighter than the natural one.
        let limits = ArtifactLimits {
            max_expansion_ratio: (natural_ratio as u64).max(2) - 1,
            ..ArtifactLimits::default()
        };
        let err = load_umls_artifact_with_limits(&path, &limits).unwrap_err();
        assert!(
            matches!(err, ArtifactError::ExpansionRatioTooLarge { .. }),
            "expected ExpansionRatioTooLarge, got: {err:?}"
        );
    }

    #[test]
    fn test_shard_count_and_aggregate_limits_enforced() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("shards.bin.zst");
        let shards = vec![vec![1u8; 32], vec![2u8; 32]]; // two shards, each 32 bytes
        let header = make_v2_header(&shards);
        save_umls_artifact(&header, &shards, &path).unwrap();

        let mut limits = ArtifactLimits {
            max_shard_count: 2,
            ..ArtifactLimits::default()
        };
        assert!(load_umls_artifact_with_limits(&path, &limits).is_ok());
        limits.max_shard_count = 1;
        let err = load_umls_artifact_with_limits(&path, &limits).unwrap_err();
        assert!(matches!(err, ArtifactError::TooManyShards { .. }));

        let mut limits = ArtifactLimits {
            max_total_shard_bytes: 64, // aggregate exactly
            ..ArtifactLimits::default()
        };
        assert!(load_umls_artifact_with_limits(&path, &limits).is_ok());
        limits.max_total_shard_bytes = 63;
        let err = load_umls_artifact_with_limits(&path, &limits).unwrap_err();
        assert!(matches!(err, ArtifactError::AggregateTooLarge { .. }));
    }

    #[test]
    fn test_zero_limits_fail_closed() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("zero.bin.zst");
        let shards = vec![vec![1u8; 8]];
        let header = make_v2_header(&shards);
        save_umls_artifact(&header, &shards, &path).unwrap();

        let limits = ArtifactLimits {
            max_compressed_bytes: 0,
            max_expanded_bytes: 0,
            max_expansion_ratio: 0,
            max_header_bytes: 0,
            max_shard_count: 0,
            max_total_shard_bytes: 0,
            max_patterns: 0,
            max_pattern_len: 0,
            max_total_pattern_bytes: 0,
        };
        let result = load_umls_artifact_with_limits(&path, &limits);
        assert!(result.is_err(), "zero limits must fail closed");
    }

    // ---------- Structural validation tests ----------

    #[test]
    fn test_trailing_bytes_rejected() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("trailing.bin.zst");
        let shards = vec![vec![1u8; 8]];
        let header = make_v2_header(&shards);
        let mut raw = build_v2_raw(&header, &shards);
        raw.extend_from_slice(b"garbage");
        write_artifact(&raw, &path);

        let err = load_umls_artifact(&path).unwrap_err();
        assert!(
            matches!(err, ArtifactError::TrailingBytes { .. }),
            "expected TrailingBytes, got: {err:?}"
        );
    }

    #[test]
    fn test_empty_artifact_rejected() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("empty.bin.zst");
        std::fs::write(&path, Vec::<u8>::new()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let err = load_umls_artifact(&path).unwrap_err();
        assert!(
            matches!(err, ArtifactError::Truncated { .. } | ArtifactError::Io(_)),
            "empty artefact must fail deterministically, got: {err:?}"
        );
    }

    #[test]
    fn test_checksum_valid_malformed_metadata_rejected() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("unsorted.bin.zst");
        // Checksums are consistent with the shard bytes, but the pattern
        // metadata violates the sorted-unique invariant required for safe
        // matcher construction. Must be rejected without any automaton work.
        let shards = vec![vec![1u8; 8]];
        let mut header = make_v2_header(&shards);
        header.shard_metadata = vec![vec![
            PatternMeta {
                cuis: vec!["C1".to_string()],
                term: "beta".to_string(),
            },
            PatternMeta {
                cuis: vec!["C2".to_string()],
                term: "alpha".to_string(),
            },
        ]];
        write_artifact(&build_v2_raw(&header, &shards), &path);

        let err = load_umls_artifact(&path).unwrap_err();
        assert!(
            matches!(err, ArtifactError::PatternsUnsorted { shard: 0 }),
            "expected PatternsUnsorted, got: {err:?}"
        );
    }

    #[test]
    fn test_duplicate_patterns_rejected() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("dupes.bin.zst");
        let shards = vec![vec![1u8; 8]];
        let mut header = make_v2_header(&shards);
        header.shard_metadata = vec![vec![
            PatternMeta {
                cuis: vec!["C1".to_string()],
                term: "alpha".to_string(),
            },
            PatternMeta {
                cuis: vec!["C2".to_string()],
                term: "alpha".to_string(),
            },
        ]];
        write_artifact(&build_v2_raw(&header, &shards), &path);

        let err = load_umls_artifact(&path).unwrap_err();
        assert!(matches!(err, ArtifactError::PatternsUnsorted { .. }));
    }

    #[test]
    fn test_total_patterns_mismatch_rejected() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("mismatch.bin.zst");
        let shards = vec![vec![1u8; 8]];
        let mut header = make_v2_header(&shards);
        header.total_patterns = 5; // metadata holds 2
        write_artifact(&build_v2_raw(&header, &shards), &path);

        let err = load_umls_artifact(&path).unwrap_err();
        assert!(matches!(err, ArtifactError::InvalidMetadata(_)));
    }

    #[test]
    fn test_pattern_budget_enforced() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("patterns.bin.zst");
        let shards = vec![vec![1u8; 8]];
        let header = make_v2_header(&shards);

        let mut limits = ArtifactLimits {
            max_patterns: 2, // exactly: allowed
            ..ArtifactLimits::default()
        };
        write_artifact(&build_v2_raw(&header, &shards), &path);
        assert!(load_umls_artifact_with_limits(&path, &limits).is_ok());

        limits.max_patterns = 1;
        let err = load_umls_artifact_with_limits(&path, &limits).unwrap_err();
        assert!(matches!(err, ArtifactError::TooManyPatterns { .. }));

        // A single overly long pattern.
        let mut header_long = make_v2_header(&shards);
        header_long.shard_metadata = vec![vec![PatternMeta {
            cuis: vec!["C1".to_string()],
            term: "x".repeat(100),
        }]];
        header_long.total_patterns = 1;
        let path2 = dir.path().join("long.bin.zst");
        write_artifact(&build_v2_raw(&header_long, &shards), &path2);
        let limits = ArtifactLimits {
            max_pattern_len: 10,
            ..ArtifactLimits::default()
        };
        let err = load_umls_artifact_with_limits(&path2, &limits).unwrap_err();
        assert!(matches!(err, ArtifactError::PatternTooLong { .. }));
    }

    #[test]
    fn test_header_size_limit_enforced() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("header.bin.zst");
        let shards = vec![vec![1u8; 8]];
        let header = make_v2_header(&shards);
        save_umls_artifact(&header, &shards, &path).unwrap();

        let limits = ArtifactLimits {
            max_header_bytes: 1, // any real header exceeds this
            ..ArtifactLimits::default()
        };
        let err = load_umls_artifact_with_limits(&path, &limits).unwrap_err();
        assert!(matches!(err, ArtifactError::HeaderTooLarge { .. }));
    }
}

// end of crate::medical_artifact
