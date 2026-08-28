use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use uuid::Uuid;

use super::{
    CURRENT_SCHEMA, LEGACY_SCHEMA, PrincipalStore, PutRejection, StorageBroker, StorageKey,
    StoragePrincipal, StorageRevision, StoreLimits, StoreState, StoredValue, UninstallDisposition,
};
use crate::harness::policy::ContainmentPolicy;
use crate::harness::report::{StorageEvidence, Verification};

struct EvidenceRoot(PathBuf);

impl Drop for EvidenceRoot {
    fn drop(&mut self) {
        // This unique manager-created directory contains only this evidence run's storage data.
        if let Err(error) = fs::remove_dir_all(&self.0)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!("failed to remove storage evidence root: {error}");
        }
    }
}

pub(in crate::harness) fn run(
    results_directory: &Path,
    policy: &ContainmentPolicy,
) -> Result<StorageEvidence> {
    let workload = policy.workload();
    let limits = StoreLimits {
        maximum_key: workload.storage_key_limit_bytes(),
        maximum_value: workload.storage_value_limit_bytes(),
        maximum_entries: workload.storage_entry_limit(),
        quota: workload.storage_quota_bytes(),
    };
    let root = EvidenceRoot(
        results_directory.join(format!("storage-evidence-{}", Uuid::new_v4().simple())),
    );
    let broker = StorageBroker::create(root.0.clone(), limits)?;
    let alpha = StoragePrincipal::parse("extension.alpha")?;
    let beta = StoragePrincipal::parse("extension.beta")?;
    verify_bounded_input(&broker, limits)?;
    verify_entry_limit(limits)?;
    let key = StorageKey::parse("theme", limits.maximum_key)?;
    let (store, initial) = migrate_with_rollback(&broker, &alpha, &key)?;
    exercise_commit_recovery(&broker, &alpha, &key, store, &initial, limits)?;
    exercise_lifecycle(&broker, &alpha, &beta, &key)?;
    fs::remove_dir(&root.0).context("remove empty storage evidence root")?;

    Ok(StorageEvidence {
        backend: "manager-private per-principal atomic snapshot",
        schema_before: LEGACY_SCHEMA,
        schema_after: CURRENT_SCHEMA,
        staged_migration: Verification::Passed,
        migration_rollback: Verification::Passed,
        cas_conflict_rejected: Verification::Passed,
        quota_enforced: Verification::Passed,
        entry_limit_enforced: Verification::Passed,
        oversized_snapshot_rejected: Verification::Passed,
        synced_stage_recovered: Verification::Passed,
        orphan_stages_removed: 1,
        uninstall_retained: Verification::Passed,
        explicit_deletion: Verification::Passed,
        cross_principal_read_denied: Verification::Passed,
        backing_path_exposed_to_child: false,
    })
}

fn verify_bounded_input(broker: &StorageBroker, limits: StoreLimits) -> Result<()> {
    let principal = StoragePrincipal::parse("extension.corrupt")?;
    let fixture_length = limits
        .maximum_snapshot_bytes()?
        .checked_add(1)
        .context("oversized storage recovery fixture length overflow")?;
    broker.install_corrupt_for_recovery_test(&principal, &vec![b' '; fixture_length])?;
    ensure!(
        broker.open(&principal).is_err(),
        "oversized storage snapshot was accepted"
    );
    broker.uninstall(&principal, UninstallDisposition::Delete)
}

fn verify_entry_limit(limits: StoreLimits) -> Result<()> {
    let mut state = StoreState::default();
    for index in 0..limits.maximum_entries {
        let key = StorageKey::parse(&format!("entry-{index}"), limits.maximum_key)?;
        state.entries.insert(
            key,
            StoredValue {
                revision: StorageRevision::INITIAL,
                value: Vec::new(),
            },
        );
    }
    let extra = StorageKey::parse("entry-overflow", limits.maximum_key)?;
    ensure!(
        matches!(
            state.plan_put(extra, None, Vec::new(), limits),
            Err(PutRejection::EntryLimit)
        ),
        "storage entry limit was not enforced"
    );
    Ok(())
}

fn migrate_with_rollback(
    broker: &StorageBroker,
    principal: &StoragePrincipal,
    key: &StorageKey,
) -> Result<(PrincipalStore, StoredValue)> {
    let mut legacy = BTreeMap::new();
    legacy.insert(key.clone(), b"nezuko-pink".to_vec());
    broker.install_legacy(principal, legacy)?;

    let migrated = broker.open(principal)?;
    ensure!(
        migrated.migration_performed,
        "legacy store was not migrated"
    );
    let initial = migrated
        .store
        .get(key)
        .context("migration lost the legacy value")?
        .clone();
    drop(migrated);
    broker.rollback_last_commit(principal)?;
    let rolled_back = broker.open(principal)?;
    ensure!(
        rolled_back.migration_performed && rolled_back.store.get(key) == Some(&initial),
        "migration rollback did not preserve the legacy value"
    );
    Ok((rolled_back.store, initial))
}

fn exercise_commit_recovery(
    broker: &StorageBroker,
    principal: &StoragePrincipal,
    key: &StorageKey,
    mut store: PrincipalStore,
    initial: &StoredValue,
    limits: StoreLimits,
) -> Result<()> {
    let revision = store.put(
        key.clone(),
        Some(initial.revision),
        b"nezuko-rose-petals".to_vec(),
    )??;
    let conflict = store.put(key.clone(), Some(initial.revision), b"stale".to_vec())?;
    ensure!(
        conflict
            == Err(PutRejection::Conflict {
                current: Some(revision)
            }),
        "stale storage CAS was not rejected"
    );
    let quota_a = StorageKey::parse("quota-a", limits.maximum_key)?;
    let quota_b = StorageKey::parse("quota-b", limits.maximum_key)?;
    store.put(quota_a, None, vec![1; limits.maximum_value])??;
    let quota = store.put(quota_b, None, vec![2; limits.maximum_value])?;
    ensure!(
        quota == Err(PutRejection::Quota),
        "storage quota was not enforced"
    );

    store.abandon_put_for_recovery_test(key.clone(), Some(revision), b"uncommitted".to_vec())?;
    drop(store);
    let recovered = broker.open(principal)?;
    ensure!(
        recovered.orphan_stages_removed == 1
            && recovered.store.get(key).map(|entry| entry.value.as_slice())
                == Some(b"nezuko-rose-petals"),
        "crash recovery did not preserve the committed value"
    );
    Ok(())
}

fn exercise_lifecycle(
    broker: &StorageBroker,
    principal: &StoragePrincipal,
    other: &StoragePrincipal,
    key: &StorageKey,
) -> Result<()> {
    broker.uninstall(principal, UninstallDisposition::Retain)?;
    let retained = broker.open(principal)?;
    ensure!(
        retained.store.get(key).is_some(),
        "uninstall retention lost data"
    );
    let isolated = broker.open(other)?;
    ensure!(
        isolated.store.get(key).is_none(),
        "principal storage crossed identity boundary"
    );
    drop((retained, isolated));
    broker.uninstall(principal, UninstallDisposition::Delete)?;
    let deleted = broker.open(principal)?;
    ensure!(
        deleted.store.get(key).is_none(),
        "explicit deletion retained data"
    );
    drop(deleted);
    broker.uninstall(principal, UninstallDisposition::Delete)?;
    broker.uninstall(other, UninstallDisposition::Delete)
}
