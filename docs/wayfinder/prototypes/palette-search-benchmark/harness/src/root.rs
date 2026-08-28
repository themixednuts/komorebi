use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::native::{
    KnownFolderKind, NativeError, RootCandidate, StableFileIdentity, file_identity, root_attributes,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullDrivePolicy {
    Reject,
    ExplicitlyAllowed,
}

#[derive(Debug, Clone)]
pub struct AdmittedRoot {
    pub kind: KnownFolderKind,
    pub path: PathBuf,
    pub identity: StableFileIdentity,
    pub ntfs_identity: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootDiagnostic {
    pub path_tag: String,
    pub kind: KnownFolderKind,
    pub outcome: RootOutcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RootOutcome {
    Admitted,
    Duplicate,
    Missing,
    FullDriveRequiresOptIn,
    ReparseRoot,
    HydrationRisk,
}

pub fn admit_roots(
    candidates: Vec<RootCandidate>,
    full_drive: FullDrivePolicy,
) -> Result<(Vec<AdmittedRoot>, Vec<RootDiagnostic>), RootError> {
    let mut admitted = Vec::new();
    let mut diagnostics = Vec::with_capacity(candidates.len());
    let mut identities = HashSet::new();

    for candidate in candidates {
        let path_tag = redact_path(&candidate.path);
        if !candidate.path.exists() {
            diagnostics.push(diagnostic(candidate.kind, path_tag, RootOutcome::Missing));
            continue;
        }
        if is_drive_root(&candidate.path) && full_drive == FullDrivePolicy::Reject {
            diagnostics.push(diagnostic(
                candidate.kind,
                path_tag,
                RootOutcome::FullDriveRequiresOptIn,
            ));
            continue;
        }
        let attributes = root_attributes(&candidate.path)?;
        if attributes.reparse {
            diagnostics.push(diagnostic(
                candidate.kind,
                path_tag,
                RootOutcome::ReparseRoot,
            ));
            continue;
        }
        if attributes.content_requires_hydration() {
            diagnostics.push(diagnostic(
                candidate.kind,
                path_tag,
                RootOutcome::HydrationRisk,
            ));
            continue;
        }
        let (identity, ntfs_identity) = file_identity(&candidate.path)?;
        if !identities.insert(identity) {
            diagnostics.push(diagnostic(candidate.kind, path_tag, RootOutcome::Duplicate));
            continue;
        }
        diagnostics.push(diagnostic(candidate.kind, path_tag, RootOutcome::Admitted));
        admitted.push(AdmittedRoot {
            kind: candidate.kind,
            path: candidate.path,
            identity,
            ntfs_identity,
        });
    }
    Ok((admitted, diagnostics))
}

fn diagnostic(kind: KnownFolderKind, path_tag: String, outcome: RootOutcome) -> RootDiagnostic {
    RootDiagnostic {
        path_tag,
        kind,
        outcome,
    }
}

#[must_use]
pub fn redact_path(path: &Path) -> String {
    let encoded = path.as_os_str().encode_wide().flat_map(u16::to_le_bytes);
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"wayfinder-palette-root-v1\0");
    for byte in encoded {
        hasher.update(&[byte]);
    }
    hasher.finalize().to_hex()[..16].to_owned()
}

fn is_drive_root(path: &Path) -> bool {
    let mut components = path.components();
    matches!(components.next(), Some(Component::Prefix(_)))
        && matches!(components.next(), Some(Component::RootDir))
        && components.next().is_none()
}

#[derive(Debug, Error)]
pub enum RootError {
    #[error("native root inspection failed")]
    Native(#[from] NativeError),
}

use std::os::windows::ffi::OsStrExt;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_never_contain_the_source_path() {
        let path = Path::new(r"C:\Users\owner\Secret Project");
        let tag = redact_path(path);
        assert_eq!(tag.len(), 16);
        assert!(!tag.contains("owner"));
        assert_eq!(tag, redact_path(path));
    }

    #[test]
    fn full_drive_requires_an_explicit_capability() {
        assert!(is_drive_root(Path::new(r"C:\")));
        assert!(!is_drive_root(Path::new(r"C:\Users")));
    }
}
