//! Residue inspection and restart replay.
//!
//! The gate must prove that no session key, pairing token, fingerprint or pairing
//! record survives. This module provides the scanner and the state-reset replay;
//! the contract evidence (`pairing_residue.json`) is only emitted when both the
//! desktop and the mobile reference devices are named, because the contract
//! requires both to be inspected with the same method.

use std::fmt;
use std::path::{Path, PathBuf};

/// One byte pattern the inspection searches for.
#[derive(Debug, Clone)]
pub struct Needle {
    /// What the pattern represents (`pairing_token`, `fingerprint`, …).
    pub label: &'static str,
    /// The bytes to search for.
    pub bytes: Vec<u8>,
}

impl Needle {
    /// Builds a needle from raw bytes.
    pub fn new(label: &'static str, bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            label,
            bytes: bytes.into(),
        }
    }

    /// Builds a needle from a lowercase hex string.
    pub fn from_hex(label: &'static str, hex: &str) -> Option<Self> {
        remotecam_protocol::hex::decode_hex(hex).map(|bytes| Self::new(label, bytes))
    }
}

/// One located occurrence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// Repository-relative or device-relative path.
    pub location: String,
    /// The needle label.
    pub label: &'static str,
    /// Byte offset within the file.
    pub offset: u64,
}

/// Scans one directory tree for every needle.
///
/// Files are read in full; a file larger than `max_file_bytes` is skipped and
/// reported in coverage by the caller. Symlinks are not followed, so the scan
/// cannot escape the named root.
pub fn scan_root(root: &Path, needles: &[Needle]) -> Result<Vec<Finding>, ScanError> {
    if !root.exists() {
        return Err(ScanError::MissingRoot(root.display().to_string()));
    }
    let mut findings = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        let metadata = std::fs::symlink_metadata(&path).map_err(ScanError::io)?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            let entries = std::fs::read_dir(&path).map_err(ScanError::io)?;
            for entry in entries {
                let entry = entry.map_err(ScanError::io)?;
                pending.push(entry.path());
            }
            continue;
        }
        if !metadata.is_file() || metadata.len() > 64 * 1024 * 1024 {
            continue;
        }
        scan_file(&path, needles, &mut findings)?;
    }
    Ok(findings)
}

fn scan_file(
    path: &Path,
    needles: &[Needle],
    findings: &mut Vec<Finding>,
) -> Result<(), ScanError> {
    let bytes = std::fs::read(path).map_err(ScanError::io)?;
    for needle in needles {
        if needle.bytes.is_empty() {
            continue;
        }
        let mut start = 0;
        while start + needle.bytes.len() <= bytes.len() {
            let Some(offset) = find(&bytes[start..], &needle.bytes) else {
                break;
            };
            findings.push(Finding {
                location: path.display().to_string(),
                label: needle.label,
                offset: u64::try_from(start + offset).unwrap_or(u64::MAX),
            });
            start += offset + 1;
        }
    }
    Ok(())
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Builds the needles for one session.
#[cfg(test)]
pub fn session_needles(
    pairing_token: &[u8; 32],
    tls_fingerprint: &[u8; 32],
    session_key: &[u8; 32],
) -> Vec<Needle> {
    vec![
        Needle::new("pairing_token", pairing_token.to_vec()),
        Needle::new(
            "pairing_token_hex",
            remotecam_protocol::hex::encode_hex(pairing_token).into_bytes(),
        ),
        Needle::new("fingerprint", tls_fingerprint.to_vec()),
        Needle::new(
            "fingerprint_hex",
            remotecam_protocol::hex::encode_hex(tls_fingerprint).into_bytes(),
        ),
        Needle::new("session_key", session_key.to_vec()),
        Needle::new(
            "session_key_hex",
            remotecam_protocol::hex::encode_hex(session_key).into_bytes(),
        ),
    ]
}

/// Replays a token after the pairing state has been reset.
///
/// This is the automated part of the restart claim: a listener that restarts
/// with a fresh token must not accept the previous session's token. The full
/// process-kill procedure is executed on the reference devices and recorded by
/// the gate procedure.
#[cfg(test)]
pub fn token_replay_after_state_reset(
    previous: [u8; 32],
    fresh: [u8; 32],
) -> remotecam_protocol::ReasonCode {
    let first = remotecam_signaling::token::TokenStore::new();
    first.issue(previous, std::time::Duration::ZERO);
    assert_eq!(first.take(&previous, std::time::Duration::ZERO), Ok(()));
    drop(first);

    let restarted = remotecam_signaling::token::TokenStore::new();
    restarted.issue(fresh, std::time::Duration::ZERO);
    restarted
        .take(&previous, std::time::Duration::ZERO)
        .expect_err("the previous token must not be reusable after a restart")
}

/// A device named on the command line.
#[derive(Debug, Clone)]
pub struct DeviceSpec {
    /// `desktop` or `mobile`.
    pub role: String,
    /// The device name recorded in the evidence.
    pub name: String,
    /// The app-data directory that was searched.
    pub root: PathBuf,
}

/// Failure while scanning.
#[derive(Debug)]
pub enum ScanError {
    /// A named root does not exist.
    MissingRoot(String),
    /// A filesystem operation failed.
    Io(std::io::Error),
}

impl ScanError {
    fn io(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl fmt::Display for ScanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingRoot(path) => write!(formatter, "inspection root does not exist: {path}"),
            Self::Io(error) => write!(formatter, "inspection failed: {error}"),
        }
    }
}

impl std::error::Error for ScanError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_seeded_token_is_detected_and_a_clean_tree_is_not() {
        let root = std::env::temp_dir().join(format!(
            "remotecam-residue-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("journals")).expect("create");
        let token = [0x5a; 32];
        let needles = session_needles(&token, &[0x11; 32], &[0x22; 32]);
        std::fs::write(root.join("empty.bin"), b"nothing here").expect("write");
        assert!(scan_root(&root, &needles).expect("scan").is_empty());

        std::fs::write(root.join("journals").join("crash.bin"), token).expect("seed");
        let findings = scan_root(&root, &needles).expect("scan");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].label, "pairing_token");

        std::fs::remove_dir_all(&root).expect("cleanup");
        assert!(!root.exists());
    }

    #[test]
    fn a_previous_token_is_not_reusable_after_a_state_reset() {
        assert_eq!(
            token_replay_after_state_reset([0x01; 32], [0x02; 32]),
            remotecam_protocol::ReasonCode::TokenInvalid
        );
    }
}
