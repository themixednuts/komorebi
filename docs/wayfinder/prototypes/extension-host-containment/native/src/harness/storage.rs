mod atomic;
mod evidence;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::num::NonZeroU64;
use std::path::PathBuf;

use self::atomic::AtomicFiles;
pub(super) use self::evidence::run;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

const LEGACY_SCHEMA: u32 = 1;
const CURRENT_SCHEMA: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct StoragePrincipal(String);

impl StoragePrincipal {
    fn parse(value: &str) -> Result<Self> {
        ensure!(
            !value.is_empty()
                && value.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')
                })
                && value != "."
                && value != "..",
            "storage principal must be a nonempty portable identifier"
        );
        Ok(Self(value.to_owned()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
struct StorageKey(String);

impl StorageKey {
    fn parse(value: &str, maximum_bytes: usize) -> Result<Self> {
        ensure!(
            !value.is_empty()
                && value.len() <= maximum_bytes
                && value.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')
                })
                && value != "."
                && value != "..",
            "storage key must be a bounded portable identifier"
        );
        Ok(Self(value.to_owned()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
struct StorageRevision(NonZeroU64);

impl StorageRevision {
    const INITIAL: Self = Self(NonZeroU64::MIN);

    fn next(self) -> Result<Self> {
        let value = self
            .0
            .get()
            .checked_add(1)
            .context("storage revision overflow")?;
        Ok(Self(
            NonZeroU64::new(value).context("next storage revision was zero")?,
        ))
    }
}

#[derive(Debug, Clone, Copy)]
struct StoreLimits {
    maximum_key: usize,
    maximum_value: usize,
    maximum_entries: usize,
    quota: usize,
}

impl StoreLimits {
    fn maximum_snapshot_bytes(self) -> Result<usize> {
        let encoded_values = self
            .quota
            .checked_mul(4)
            .context("encoded storage quota overflow")?;
        let entry_overhead = self
            .maximum_key
            .checked_add(128)
            .and_then(|bytes| bytes.checked_mul(self.maximum_entries))
            .context("encoded storage entry limit overflow")?;
        encoded_values
            .checked_add(entry_overhead)
            .and_then(|bytes| bytes.checked_add(1024))
            .context("encoded storage snapshot limit overflow")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct StoredValue {
    revision: StorageRevision,
    value: Vec<u8>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct StoreState {
    generation: u64,
    entries: BTreeMap<StorageKey, StoredValue>,
}

impl StoreState {
    fn plan_put(
        &self,
        key: StorageKey,
        expected: Option<StorageRevision>,
        value: Vec<u8>,
        limits: StoreLimits,
    ) -> std::result::Result<PutPlan, PutRejection> {
        let current = self.entries.get(&key);
        if current.map(|entry| entry.revision) != expected {
            return Err(PutRejection::Conflict {
                current: current.map(|entry| entry.revision),
            });
        }
        if value.len() > limits.maximum_value {
            return Err(PutRejection::ValueLimit);
        }
        if current.is_none() && self.entries.len() >= limits.maximum_entries {
            return Err(PutRejection::EntryLimit);
        }
        let used = self
            .entries
            .values()
            .try_fold(0_usize, |total, entry| total.checked_add(entry.value.len()));
        let Some(used) = used else {
            return Err(PutRejection::SizeOverflow);
        };
        let previous = current.map_or(0, |entry| entry.value.len());
        let Some(next_used) = used
            .checked_sub(previous)
            .and_then(|remaining| remaining.checked_add(value.len()))
        else {
            return Err(PutRejection::SizeOverflow);
        };
        if next_used > limits.quota {
            return Err(PutRejection::Quota);
        }
        let revision = match current {
            Some(entry) => entry
                .revision
                .next()
                .map_err(|_| PutRejection::RevisionOverflow)?,
            None => StorageRevision::INITIAL,
        };
        let Some(generation) = self.generation.checked_add(1) else {
            return Err(PutRejection::RevisionOverflow);
        };
        let mut next = self.clone();
        next.generation = generation;
        next.entries.insert(key, StoredValue { revision, value });
        Ok(PutPlan { next, revision })
    }

    fn validate(&self, limits: StoreLimits) -> Result<()> {
        ensure!(
            self.entries.len() <= limits.maximum_entries,
            "stored entry count exceeds configured limit"
        );
        let mut used = 0_usize;
        for (key, entry) in &self.entries {
            StorageKey::parse(&key.0, limits.maximum_key)?;
            ensure!(
                entry.value.len() <= limits.maximum_value,
                "stored value exceeds configured limit"
            );
            used = used
                .checked_add(entry.value.len())
                .context("stored byte count overflow")?;
        }
        ensure!(used <= limits.quota, "stored values exceed principal quota");
        Ok(())
    }
}

struct PutPlan {
    next: StoreState,
    revision: StorageRevision,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PutRejection {
    Conflict { current: Option<StorageRevision> },
    ValueLimit,
    EntryLimit,
    Quota,
    SizeOverflow,
    RevisionOverflow,
}

impl fmt::Display for PutRejection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for PutRejection {}

#[derive(Serialize, Deserialize)]
struct DiskStoreV1 {
    schema_version: u32,
    entries: BTreeMap<StorageKey, Vec<u8>>,
}

#[derive(Serialize, Deserialize)]
struct DiskStoreV2 {
    schema_version: u32,
    generation: u64,
    entries: BTreeMap<StorageKey, StoredValue>,
}

struct DecodedStore {
    state: StoreState,
    source_schema: u32,
}

fn decode(bytes: &[u8], limits: StoreLimits) -> Result<DecodedStore> {
    #[derive(Deserialize)]
    struct Header {
        schema_version: u32,
    }

    let header: Header = serde_json::from_slice(bytes).context("read storage schema header")?;
    let decoded = match header.schema_version {
        LEGACY_SCHEMA => {
            let legacy: DiskStoreV1 =
                serde_json::from_slice(bytes).context("decode legacy extension store")?;
            let entries = legacy
                .entries
                .into_iter()
                .map(|(key, value)| {
                    (
                        key,
                        StoredValue {
                            revision: StorageRevision::INITIAL,
                            value,
                        },
                    )
                })
                .collect();
            DecodedStore {
                state: StoreState {
                    generation: 1,
                    entries,
                },
                source_schema: LEGACY_SCHEMA,
            }
        }
        CURRENT_SCHEMA => {
            let current: DiskStoreV2 =
                serde_json::from_slice(bytes).context("decode current extension store")?;
            DecodedStore {
                state: StoreState {
                    generation: current.generation,
                    entries: current.entries,
                },
                source_schema: CURRENT_SCHEMA,
            }
        }
        version => anyhow::bail!("unsupported extension storage schema {version}"),
    };
    decoded.state.validate(limits)?;
    Ok(decoded)
}

fn encode(state: &StoreState) -> Result<Vec<u8>> {
    serde_json::to_vec(&DiskStoreV2 {
        schema_version: CURRENT_SCHEMA,
        generation: state.generation,
        entries: state.entries.clone(),
    })
    .context("encode current extension store")
}

struct StorageBroker {
    root: PathBuf,
    limits: StoreLimits,
}

impl StorageBroker {
    fn create(root: PathBuf, limits: StoreLimits) -> Result<Self> {
        ensure!(limits.maximum_value <= limits.quota);
        fs::create_dir_all(&root).context("create broker storage root")?;
        Ok(Self { root, limits })
    }

    fn open(&self, principal: &StoragePrincipal) -> Result<OpenedStore> {
        let files = AtomicFiles::create(self.root.join(&principal.0))?;
        let orphan_stages_removed = files.remove_orphan_stages()?;
        let decoded = match files.read_active(self.limits.maximum_snapshot_bytes()?)? {
            Some(bytes) => decode(&bytes, self.limits)?,
            None => DecodedStore {
                state: StoreState::default(),
                source_schema: CURRENT_SCHEMA,
            },
        };
        let migration_performed = decoded.source_schema != CURRENT_SCHEMA;
        if migration_performed {
            files.stage(&encode(&decoded.state)?)?.promote(&files)?;
        }
        Ok(OpenedStore {
            store: PrincipalStore {
                files,
                limits: self.limits,
                state: decoded.state,
            },
            migration_performed,
            orphan_stages_removed,
        })
    }

    fn install_legacy(
        &self,
        principal: &StoragePrincipal,
        entries: BTreeMap<StorageKey, Vec<u8>>,
    ) -> Result<()> {
        let files = AtomicFiles::create(self.root.join(&principal.0))?;
        let bytes = serde_json::to_vec(&DiskStoreV1 {
            schema_version: LEGACY_SCHEMA,
            entries,
        })?;
        files.stage(&bytes)?.promote(&files)
    }

    fn install_corrupt_for_recovery_test(
        &self,
        principal: &StoragePrincipal,
        bytes: &[u8],
    ) -> Result<()> {
        let files = AtomicFiles::create(self.root.join(&principal.0))?;
        files.stage(bytes)?.promote(&files)
    }

    fn rollback_last_commit(&self, principal: &StoragePrincipal) -> Result<()> {
        AtomicFiles::create(self.root.join(&principal.0))?.rollback()
    }

    fn uninstall(
        &self,
        principal: &StoragePrincipal,
        disposition: UninstallDisposition,
    ) -> Result<()> {
        if disposition == UninstallDisposition::Retain {
            return Ok(());
        }
        let directory = self.root.join(&principal.0);
        match fs::remove_dir_all(directory) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error).context("delete principal storage"),
        }
    }
}

struct OpenedStore {
    store: PrincipalStore,
    migration_performed: bool,
    orphan_stages_removed: usize,
}

struct PrincipalStore {
    files: AtomicFiles,
    limits: StoreLimits,
    state: StoreState,
}

impl PrincipalStore {
    fn get(&self, key: &StorageKey) -> Option<&StoredValue> {
        self.state.entries.get(key)
    }

    fn put(
        &mut self,
        key: StorageKey,
        expected: Option<StorageRevision>,
        value: Vec<u8>,
    ) -> Result<std::result::Result<StorageRevision, PutRejection>> {
        let plan = match self.state.plan_put(key, expected, value, self.limits) {
            Ok(plan) => plan,
            Err(rejection) => return Ok(Err(rejection)),
        };
        self.files
            .stage(&encode(&plan.next)?)?
            .promote(&self.files)?;
        self.state = plan.next;
        Ok(Ok(plan.revision))
    }

    fn abandon_put_for_recovery_test(
        &self,
        key: StorageKey,
        expected: Option<StorageRevision>,
        value: Vec<u8>,
    ) -> Result<()> {
        let plan = self
            .state
            .plan_put(key, expected, value, self.limits)
            .map_err(|rejection| anyhow::anyhow!(rejection))?;
        self.files
            .stage(&encode(&plan.next)?)?
            .abandon_for_recovery_test();
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UninstallDisposition {
    Retain,
    Delete,
}
