use std::fs;
use std::io::Write;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;
use thiserror::Error;
use windows_sys::Win32::Storage::FileSystem::{
    MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
};

use crate::domain::{FaultProfile, InstallationId, PromotionIdentity};
use crate::native_path::{NativePathError, to_wide_null};
use crate::store::{Store, StoreError};

#[derive(Clone, Debug)]
pub struct Layout {
    root: PathBuf,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WindowSnapshot {
    pub windows: Vec<WindowPlacement>,
    pub focused: String,
    pub appearance_digest: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WindowPlacement {
    pub identity: String,
    pub frame: [i32; 4],
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MigratedConfiguration {
    pub schema: u8,
    pub workspaces: Vec<String>,
    pub bindings: Vec<u8>,
    pub source_digest: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SourceConfiguration {
    schema: u8,
    workspaces: Vec<String>,
    bindings: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CandidateSeal {
    pub installation: InstallationId,
    pub payload_digest: String,
    pub configuration_digest: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeState {
    Live,
    Stopped,
}

impl RuntimeState {
    fn as_bytes(self) -> &'static [u8] {
        match self {
            Self::Live => b"live\n",
            Self::Stopped => b"stopped\n",
        }
    }
}

impl Layout {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn store_path(&self) -> PathBuf {
        if self.root.as_os_str().to_str().is_some() {
            self.root.join("manager-state.sqlite3")
        } else {
            self.root.parent().unwrap_or(&self.root).join(format!(
                "manager-state-{}.sqlite3",
                native_path_key(&self.root)
            ))
        }
    }

    pub fn process_runtime(&self) -> PathBuf {
        self.root.join("candidate-runtime")
    }

    pub fn candidate_socket(&self) -> PathBuf {
        let endpoint = native_path_key(&self.root);
        self.candidate_socket_parent()
            .join(format!("{endpoint}.sock"))
    }

    pub fn candidate_socket_parent(&self) -> PathBuf {
        std::env::temp_dir().join("kwp-ipc")
    }

    pub fn initialize(&self, fault: FaultProfile) -> Result<PromotionIdentity, InstallError> {
        fs::create_dir_all(self.root.join("installations")).map_err(InstallError::CreateRoot)?;
        fs::create_dir_all(self.process_runtime()).map_err(InstallError::CreateRoot)?;

        let prior = "known-good".parse().map_err(InstallError::PriorId)?;
        let candidate = "candidate-2".parse().map_err(InstallError::CandidateId)?;
        self.write_installation_payload(&prior, b"known-good-manager-v1\n")?;
        atomic_write(&self.active_pointer(), prior.as_str().as_bytes())?;
        atomic_write(&self.input_state(), RuntimeState::Live.as_bytes())?;
        atomic_write(&self.shell_state(), RuntimeState::Live.as_bytes())?;
        atomic_write(&self.effects_state(), b"clean\n")?;
        atomic_write(&self.explorer_state(), b"explorer-present\n")?;
        atomic_write(
            &self.appearance_state(),
            b"owner-theme\0owner-wallpaper\x000,40,3440,1440",
        )?;

        let identity = PromotionIdentity {
            transaction: "promotion-fixture-1".to_owned(),
            prior,
            candidate,
            fault,
        };
        Store::open(&self.store_path())?.put_native_path(&identity.transaction, self.root())?;
        Ok(identity)
    }

    pub fn seal_candidate(
        &self,
        identity: &PromotionIdentity,
    ) -> Result<CandidateSeal, InstallError> {
        self.write_installation_payload(&identity.candidate, b"candidate-manager-v2\n")?;
        let payload_digest = file_digest(&self.payload_path(&identity.candidate))?;
        let configuration_digest = source_configuration_digest(identity.fault)?;
        let seal = CandidateSeal {
            installation: identity.candidate.clone(),
            payload_digest,
            configuration_digest,
        };
        Store::open(&self.store_path())?.put_candidate_seal(&identity.transaction, &seal)?;
        Ok(seal)
    }

    pub fn verify_candidate_seal(
        &self,
        identity: &PromotionIdentity,
    ) -> Result<CandidateSeal, InstallError> {
        let store = Store::open(&self.store_path())?;
        let seal = store.candidate_seal(&identity.transaction)?;
        let payload = file_digest(&self.payload_path(&seal.installation))?;
        let configuration = store.configuration(&identity.transaction)?;
        if payload != seal.payload_digest
            || configuration.source_digest != seal.configuration_digest
        {
            return Err(InstallError::SealChanged);
        }
        Ok(seal)
    }

    pub fn migrate_configuration(
        &self,
        identity: &PromotionIdentity,
    ) -> Result<MigratedConfiguration, InstallError> {
        let source = source_configuration(identity.fault);
        if source.schema != 1 || source.workspaces.is_empty() {
            return Err(InstallError::InvalidConfiguration(
                "schema must be 1 and at least one workspace must exist",
            ));
        }
        let workspace_count = u8::try_from(source.workspaces.len())
            .map_err(|_| InstallError::InvalidConfiguration("workspace count exceeds u8"))?;
        if source
            .bindings
            .iter()
            .any(|binding| *binding >= workspace_count)
        {
            return Err(InstallError::InvalidConfiguration(
                "binding references a missing workspace",
            ));
        }
        let migrated = MigratedConfiguration {
            schema: 2,
            workspaces: source.workspaces,
            bindings: source.bindings,
            source_digest: source_configuration_digest(identity.fault)?,
        };
        let mut store = Store::open(&self.store_path())?;
        store.put_configuration(&identity.transaction, &migrated)?;
        Ok(migrated)
    }

    pub fn capture_windows(
        &self,
        identity: &PromotionIdentity,
    ) -> Result<WindowSnapshot, InstallError> {
        let appearance = fs::read(self.appearance_state()).map_err(InstallError::Read)?;
        let snapshot = WindowSnapshot {
            windows: vec![
                WindowPlacement {
                    identity: "fixture-window-1".to_owned(),
                    frame: [0, 40, 1720, 1400],
                },
                WindowPlacement {
                    identity: "fixture-window-2".to_owned(),
                    frame: [1720, 40, 1720, 1400],
                },
            ],
            focused: "fixture-window-2".to_owned(),
            appearance_digest: hex_digest(&appearance),
        };
        let mut store = Store::open(&self.store_path())?;
        store.put_window_snapshot(&identity.transaction, &snapshot)?;
        Ok(snapshot)
    }

    pub fn verify_windows_and_appearance(
        &self,
        identity: &PromotionIdentity,
    ) -> Result<(), InstallError> {
        let store = Store::open(&self.store_path())?;
        let snapshot = store.window_snapshot(&identity.transaction)?;
        let appearance = fs::read(self.appearance_state()).map_err(InstallError::Read)?;
        if snapshot.appearance_digest != hex_digest(&appearance) {
            return Err(InstallError::AppearanceChanged);
        }
        store.mark_snapshot_reconciled(&identity.transaction)?;
        Ok(())
    }

    pub fn switch_active(&self, installation: &InstallationId) -> Result<(), InstallError> {
        atomic_write(&self.active_pointer(), installation.as_str().as_bytes())
    }

    pub fn active(&self) -> Result<InstallationId, InstallError> {
        let bytes = fs::read(self.active_pointer()).map_err(InstallError::Read)?;
        let value = std::str::from_utf8(&bytes).map_err(InstallError::ActiveEncoding)?;
        value.parse().map_err(InstallError::ActiveId)
    }

    pub fn set_input(&self, state: RuntimeState) -> Result<(), InstallError> {
        atomic_write(&self.input_state(), state.as_bytes())
    }

    pub fn set_shell(&self, state: RuntimeState) -> Result<(), InstallError> {
        atomic_write(&self.shell_state(), state.as_bytes())
    }

    pub fn set_effects_clean(&self) -> Result<(), InstallError> {
        atomic_write(&self.effects_state(), b"clean\n")
    }

    pub fn mark_candidate_started(&self) -> Result<(), InstallError> {
        atomic_write(&self.process_state(), b"candidate-live\n")
    }

    pub fn mark_prior_started(&self) -> Result<(), InstallError> {
        atomic_write(&self.process_state(), b"prior-live\n")
    }

    pub fn mark_processes_stopped(&self) -> Result<(), InstallError> {
        atomic_write(&self.process_state(), b"stopped\n")
    }

    pub fn cleanup_snapshot(&self, identity: &PromotionIdentity) -> Result<(), InstallError> {
        let mut store = Store::open(&self.store_path())?;
        store.delete_window_snapshot(&identity.transaction)?;
        Ok(())
    }

    pub fn verify_convergence(
        &self,
        expected: crate::domain::Convergence,
        identity: &PromotionIdentity,
    ) -> Result<(), InstallError> {
        use crate::domain::Convergence;

        let active = self.active()?;
        let input = fs::read(self.input_state()).map_err(InstallError::Read)?;
        let shell = fs::read(self.shell_state()).map_err(InstallError::Read)?;
        let effects = fs::read(self.effects_state()).map_err(InstallError::Read)?;
        let explorer = fs::read(self.explorer_state()).map_err(InstallError::Read)?;
        let stored_root = Store::open(&self.store_path())?.native_path(&identity.transaction)?;
        let stored_units = stored_root.encode_wide();
        let current_units = self.root.as_os_str().encode_wide();
        if !stored_units.eq(current_units) {
            return Err(InstallError::NativePathChanged);
        }
        let valid = match expected {
            Convergence::Candidate => {
                active == identity.candidate && input == b"live\n" && shell == b"live\n"
            }
            Convergence::Prior | Convergence::StagingRejected => {
                active == identity.prior && input == b"live\n" && shell == b"live\n"
            }
            Convergence::SafeStopped => {
                active == identity.prior && input == b"stopped\n" && shell == b"stopped\n"
            }
        };
        if !valid || effects != b"clean\n" || explorer != b"explorer-present\n" {
            return Err(InstallError::WrongConvergence { expected, active });
        }
        Ok(())
    }

    fn write_installation_payload(
        &self,
        installation: &InstallationId,
        payload: &[u8],
    ) -> Result<(), InstallError> {
        let directory = self.root.join("installations").join(installation.as_str());
        fs::create_dir_all(&directory).map_err(InstallError::CreateRoot)?;
        atomic_write(&directory.join("manager.bin"), payload)
    }

    fn payload_path(&self, installation: &InstallationId) -> PathBuf {
        self.root
            .join("installations")
            .join(installation.as_str())
            .join("manager.bin")
    }

    fn active_pointer(&self) -> PathBuf {
        self.root.join("active.ref")
    }

    fn appearance_state(&self) -> PathBuf {
        self.root.join("appearance.native-state")
    }

    fn input_state(&self) -> PathBuf {
        self.root.join("input.state")
    }

    fn shell_state(&self) -> PathBuf {
        self.root.join("shell.state")
    }

    fn effects_state(&self) -> PathBuf {
        self.root.join("effects.state")
    }

    fn explorer_state(&self) -> PathBuf {
        self.root.join("explorer.state")
    }

    fn process_state(&self) -> PathBuf {
        self.root.join("process.state")
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), InstallError> {
    let parent = path
        .parent()
        .ok_or_else(|| InstallError::NoParent(path.to_owned()))?;
    fs::create_dir_all(parent).map_err(InstallError::CreateRoot)?;
    let mut temporary = NamedTempFile::new_in(parent).map_err(InstallError::CreateTemporary)?;
    temporary
        .write_all(bytes)
        .map_err(InstallError::WriteTemporary)?;
    temporary.as_file().sync_all().map_err(InstallError::Sync)?;
    let (_, temporary_path) = temporary.keep().map_err(InstallError::KeepTemporary)?;
    let source = to_wide_null(&temporary_path).map_err(InstallError::NativePath)?;
    let destination = to_wide_null(path).map_err(InstallError::NativePath)?;
    // SAFETY: both buffers are NUL-terminated, remain live for the call, and name files on one volume.
    let moved = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if moved == 0 {
        let move_error = std::io::Error::last_os_error();
        return match fs::remove_file(&temporary_path) {
            Ok(()) => Err(InstallError::Replace(move_error)),
            Err(cleanup) => Err(InstallError::ReplaceAndCleanup {
                replace: move_error,
                cleanup,
            }),
        };
    }
    Ok(())
}

fn file_digest(path: &Path) -> Result<String, InstallError> {
    let bytes = fs::read(path).map_err(InstallError::Read)?;
    Ok(hex_digest(&bytes))
}

fn source_configuration(fault: FaultProfile) -> SourceConfiguration {
    SourceConfiguration {
        schema: 1,
        workspaces: vec!["one".to_owned(), "two".to_owned()],
        bindings: if fault == FaultProfile::InvalidConfiguration {
            vec![0, 9]
        } else {
            vec![0, 1]
        },
    }
}

fn source_configuration_digest(fault: FaultProfile) -> Result<String, InstallError> {
    let source = source_configuration(fault);
    let mut digest = Sha256::new();
    digest.update([source.schema]);
    for workspace in source.workspaces {
        let length = u64::try_from(workspace.len()).map_err(|_| InstallError::DigestRange)?;
        digest.update(length.to_le_bytes());
        digest.update(workspace.as_bytes());
    }
    let binding_count =
        u64::try_from(source.bindings.len()).map_err(|_| InstallError::DigestRange)?;
    digest.update(binding_count.to_le_bytes());
    digest.update(source.bindings);
    Ok(hex::encode(digest.finalize()))
}

fn hex_digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn native_path_key(path: &Path) -> String {
    let mut digest = Sha256::new();
    for unit in path.as_os_str().encode_wide() {
        digest.update(unit.to_le_bytes());
    }
    hex::encode(digest.finalize()).chars().take(32).collect()
}

#[derive(Debug, Error)]
pub enum InstallError {
    #[error("create fixture directory")]
    CreateRoot(#[source] std::io::Error),
    #[error("create atomic-write temporary file")]
    CreateTemporary(#[source] std::io::Error),
    #[error("write atomic-write temporary file")]
    WriteTemporary(#[source] std::io::Error),
    #[error("sync atomic-write temporary file")]
    Sync(#[source] std::io::Error),
    #[error("retain atomic-write temporary file")]
    KeepTemporary(#[source] tempfile::PersistError),
    #[error("replace destination with synced temporary file")]
    Replace(#[source] std::io::Error),
    #[error("replace destination and clean temporary file")]
    ReplaceAndCleanup {
        #[source]
        replace: std::io::Error,
        cleanup: std::io::Error,
    },
    #[error("operational path is invalid")]
    NativePath(#[source] NativePathError),
    #[error("path has no parent: {0:?}")]
    NoParent(PathBuf),
    #[error("read fixture state")]
    Read(#[source] std::io::Error),
    #[error("durable store operation")]
    Store(#[from] StoreError),
    #[error("configuration input is too large to digest")]
    DigestRange,
    #[error("invalid configuration: {0}")]
    InvalidConfiguration(&'static str),
    #[error("candidate changed after sealing")]
    SealChanged,
    #[error("appearance changed during cutover")]
    AppearanceChanged,
    #[error("native installation path changed during promotion")]
    NativePathChanged,
    #[error("active installation reference is not UTF-8")]
    ActiveEncoding(#[source] std::str::Utf8Error),
    #[error("active installation reference is invalid")]
    ActiveId(#[source] crate::domain::InvalidInstallationId),
    #[error("fixture prior installation identifier is invalid")]
    PriorId(#[source] crate::domain::InvalidInstallationId),
    #[error("fixture candidate installation identifier is invalid")]
    CandidateId(#[source] crate::domain::InvalidInstallationId),
    #[error("expected {expected:?} convergence, found active installation {active}")]
    WrongConvergence {
        expected: crate::domain::Convergence,
        active: InstallationId,
    },
}
