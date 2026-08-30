use std::error::Error;
use std::num::NonZeroU16;
use std::num::NonZeroU32;
#[cfg(windows)]
use std::os::windows::ffi::OsStringExt;

use komorebi_command_store::CompactionDecision;
use komorebi_command_store::DispatchState;
use komorebi_command_store::DurableInvocationLedger;
use komorebi_command_store::InvocationCommitDecision;
use komorebi_command_store::InvocationInspection;
use komorebi_command_store::LeaseDecision;
use komorebi_command_store::LeaseRequest;
use komorebi_command_store::LedgerError;
use komorebi_command_store::LedgerTimestamp;
use komorebi_command_store::MINIMUM_TERMINAL_RETENTION;
use komorebi_command_store::NamespaceRegistration;
use komorebi_command_store::NewLeaseDecision;
use komorebi_command_store::OutcomeDocument;
use komorebi_command_store::RecoveryPolicy;
use komorebi_command_store::StatusDecision;
use komorebi_command_store::TerminalRecord;
use komorebi_command_store::TerminalRetention;
use komorebi_command_store::TransitionDecision;
use komorebi_protocol::CancelInvocationReply;
use komorebi_protocol::InvocationId;
use komorebi_protocol::InvocationNamespaceId;
use komorebi_protocol::InvocationProgress;
use komorebi_protocol::InvocationSequence;
use komorebi_protocol::InvocationStatusCodec;
use komorebi_protocol::InvocationStatusReply;
use komorebi_protocol::InvocationTerminal;
use komorebi_protocol::InvocationUnavailable;
use komorebi_protocol::PrincipalId;
use komorebi_protocol::SettledInvocationKind;

fn canonical_invocation(
    id: InvocationId,
    value: u8,
) -> Result<komorebi_protocol::ActionInvocation, Box<dyn Error>> {
    let epoch = komorebi_protocol::ManagerEpoch::new([4; 16])?;
    let revision = komorebi_protocol::Revision::try_from(1)?;
    let arguments = komorebi_protocol::ActionArguments::new(std::collections::BTreeMap::from([(
        komorebi_protocol::ParameterId::parse("enabled")?,
        komorebi_protocol::ActionArgument::Scalar(komorebi_protocol::ArgumentScalar::Unsigned(
            u64::from(value),
        )),
    )]))?;
    Ok(komorebi_protocol::ActionInvocation::new(
        id,
        komorebi_protocol::OfferRef::new(
            komorebi_protocol::ActionKey::new(
                komorebi_protocol::ActionId::parse("set-enabled")?,
                komorebi_protocol::ActionSchemaVersion::new(NonZeroU16::MIN),
            ),
            komorebi_protocol::ActionContractFingerprint::new([5; 32]),
            komorebi_protocol::CatalogStamp::new(epoch, revision, revision, revision),
        ),
        komorebi_protocol::StateStamp::new(epoch, revision),
        arguments,
        None,
    ))
}

fn committed_state() -> Result<komorebi_protocol::StateStamp, Box<dyn Error>> {
    Ok(komorebi_protocol::StateStamp::new(
        komorebi_protocol::ManagerEpoch::new([4; 16])?,
        komorebi_protocol::Revision::try_from(2)?,
    ))
}

struct IdentityFixture {
    principal: PrincipalId,
    other_principal: PrincipalId,
    namespace: InvocationNamespaceId,
}

impl IdentityFixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            principal: PrincipalId::new([1; 32])?,
            other_principal: PrincipalId::new([2; 32])?,
            namespace: InvocationNamespaceId::new([3; 16])?,
        })
    }

    fn id(&self, sequence: u64) -> Result<InvocationId, Box<dyn Error>> {
        Ok(InvocationId::new(
            self.namespace,
            InvocationSequence::try_from(sequence)?,
        ))
    }
}

fn commit(
    ledger: &mut DurableInvocationLedger,
    fixture: &IdentityFixture,
    id: InvocationId,
    value: u8,
    at: i64,
) -> Result<InvocationCommitDecision, Box<dyn Error>> {
    Ok(ledger.commit_invocation(
        fixture.principal,
        &canonical_invocation(id, value)?,
        committed_state()?,
        RecoveryPolicy::NeverReplay,
        LedgerTimestamp::from_unix_millis(at)?,
    )?)
}

fn open_registered(
    path: &std::path::Path,
    fixture: &IdentityFixture,
) -> Result<DurableInvocationLedger, Box<dyn Error>> {
    let mut ledger = DurableInvocationLedger::open(path)?;
    assert!(matches!(
        ledger.register_namespace(fixture.namespace, fixture.principal)?,
        NamespaceRegistration::Registered | NamespaceRegistration::Existing
    ));
    Ok(ledger)
}

fn lease_one(
    ledger: &mut DurableInvocationLedger,
    fixture: &IdentityFixture,
) -> Result<(), Box<dyn Error>> {
    assert!(matches!(
        ledger.lease(LeaseRequest {
            namespace: fixture.namespace,
            principal: fixture.principal,
            count: NonZeroU32::MIN,
        })?,
        LeaseDecision::Issued(_)
    ));
    Ok(())
}

#[test]
fn new_namespace_registration_and_first_lease_are_one_transaction() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("new-lease.sqlite");
    let fixture = IdentityFixture::new()?;
    let request = LeaseRequest {
        namespace: fixture.namespace,
        principal: fixture.principal,
        count: NonZeroU32::new(2).ok_or("nonzero count")?,
    };
    let mut ledger = DurableInvocationLedger::open(&path)?;

    let first = match ledger.lease_new(request)? {
        NewLeaseDecision::Issued(lease) => lease,
        NewLeaseDecision::NamespaceCollision => return Err("fresh namespace collided".into()),
    };
    assert!(first.contains(fixture.id(1)?));
    assert!(first.contains(fixture.id(2)?));
    assert_eq!(
        ledger.lease_new(request)?,
        NewLeaseDecision::NamespaceCollision
    );

    drop(ledger);
    let mut reopened = DurableInvocationLedger::open(&path)?;
    let next = match reopened.lease(LeaseRequest {
        count: NonZeroU32::MIN,
        ..request
    })? {
        LeaseDecision::Issued(lease) => lease,
        other => return Err(format!("existing namespace lease failed: {other:?}").into()),
    };
    assert_eq!(next.first(), InvocationSequence::try_from(3)?);
    Ok(())
}

#[test]
fn atomic_invocation_commit_uses_one_canonical_representation() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let fixture = IdentityFixture::new()?;
    let id = fixture.id(1)?;
    let mut ledger = open_registered(&directory.path().join("typed.sqlite"), &fixture)?;
    lease_one(&mut ledger, &fixture)?;
    let invocation = canonical_invocation(id, 1)?;

    assert!(matches!(
        ledger.commit_invocation(
            fixture.principal,
            &invocation,
            committed_state()?,
            RecoveryPolicy::NeverReplay,
            LedgerTimestamp::from_unix_millis(1)?,
        )?,
        InvocationCommitDecision::Committed(_)
    ));
    assert!(matches!(
        ledger.commit_invocation(
            fixture.principal,
            &invocation,
            committed_state()?,
            RecoveryPolicy::NeverReplay,
            LedgerTimestamp::from_unix_millis(2)?,
        )?,
        InvocationCommitDecision::Retained(_)
    ));
    assert_eq!(
        ledger.commit_invocation(
            fixture.principal,
            &canonical_invocation(id, 0)?,
            committed_state()?,
            RecoveryPolicy::NeverReplay,
            LedgerTimestamp::from_unix_millis(3)?,
        )?,
        InvocationCommitDecision::IdempotencyConflict
    );
    Ok(())
}

#[test]
fn rejected_atomic_commit_does_not_consume_the_invocation_identity() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let fixture = IdentityFixture::new()?;
    let id = fixture.id(1)?;
    let mut ledger = open_registered(&directory.path().join("rollback.sqlite"), &fixture)?;
    lease_one(&mut ledger, &fixture)?;
    let invocation = canonical_invocation(id, 1)?;
    let wrong_epoch =
        komorebi_protocol::StateStamp::initial(komorebi_protocol::ManagerEpoch::new([9; 16])?);

    assert!(matches!(
        ledger.commit_invocation(
            fixture.principal,
            &invocation,
            wrong_epoch,
            RecoveryPolicy::NeverReplay,
            LedgerTimestamp::from_unix_millis(1)?,
        ),
        Err(LedgerError::CommitStateMismatch)
    ));
    assert_eq!(
        ledger.inspect_invocation(fixture.principal, &invocation)?,
        InvocationInspection::Vacant
    );
    assert!(matches!(
        ledger.commit_invocation(
            fixture.principal,
            &invocation,
            committed_state()?,
            RecoveryPolicy::NeverReplay,
            LedgerTimestamp::from_unix_millis(2)?,
        )?,
        InvocationCommitDecision::Committed(_)
    ));
    Ok(())
}

#[test]
fn idempotency_inspection_does_not_consume_a_vacant_identity() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let fixture = IdentityFixture::new()?;
    let id = fixture.id(1)?;
    let mut ledger = open_registered(&directory.path().join("inspect.sqlite"), &fixture)?;
    lease_one(&mut ledger, &fixture)?;
    let invocation = canonical_invocation(id, 1)?;

    assert_eq!(
        ledger.inspect_invocation(fixture.principal, &invocation)?,
        InvocationInspection::Vacant
    );
    let committed = ledger.commit_invocation(
        fixture.principal,
        &invocation,
        committed_state()?,
        RecoveryPolicy::NeverReplay,
        LedgerTimestamp::from_unix_millis(1)?,
    )?;
    assert!(matches!(committed, InvocationCommitDecision::Committed(_)));
    assert!(matches!(
        ledger.inspect_invocation(fixture.principal, &invocation)?,
        InvocationInspection::Retained(_)
    ));
    assert_eq!(
        ledger.inspect_invocation(fixture.principal, &canonical_invocation(id, 2)?)?,
        InvocationInspection::IdempotencyConflict
    );
    assert_eq!(
        ledger.inspect_invocation(fixture.other_principal, &invocation)?,
        InvocationInspection::IdempotencyConflict
    );
    Ok(())
}

#[test]
fn terminal_status_survives_reopen_on_a_unicode_windows_path() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("台帳-💮.sqlite");
    let fixture = IdentityFixture::new()?;
    let id = fixture.id(1)?;
    let mut ledger = open_registered(&path, &fixture)?;
    lease_one(&mut ledger, &fixture)?;

    let committed = match commit(&mut ledger, &fixture, id, 7, 1)? {
        InvocationCommitDecision::Committed(committed) => committed,
        other => return Err(format!("expected commit, got {other:?}").into()),
    };
    assert!(matches!(
        commit(&mut ledger, &fixture, id, 7, 1)?,
        InvocationCommitDecision::Retained(_)
    ));
    assert_eq!(
        commit(&mut ledger, &fixture, id, 8, 1)?,
        InvocationCommitDecision::IdempotencyConflict
    );
    assert_eq!(
        ledger.commit_invocation(
            fixture.other_principal,
            &canonical_invocation(id, 7)?,
            committed_state()?,
            RecoveryPolicy::NeverReplay,
            LedgerTimestamp::from_unix_millis(1)?,
        )?,
        InvocationCommitDecision::IdempotencyConflict
    );
    assert_eq!(
        ledger.mark_effect_dispatched(
            committed.invocation_id(),
            LedgerTimestamp::from_unix_millis(3)?,
        )?,
        TransitionDecision::Applied
    );
    let outcome = OutcomeDocument::new(NonZeroU16::MIN, [4])?;
    assert_eq!(
        ledger.record_terminal(
            id,
            TerminalRecord {
                kind: SettledInvocationKind::Succeeded,
                outcome: outcome.clone(),
                recorded_at: LedgerTimestamp::from_unix_millis(4)?,
            },
        )?,
        TransitionDecision::Applied
    );
    drop(ledger);

    let reopened = DurableInvocationLedger::open(&path)?;
    let decision = reopened.status(fixture.principal, id)?;
    assert!(matches!(
        decision.clone().into_reply(),
        InvocationStatusReply::Retained(status)
            if status.progress()
                == InvocationProgress::Terminal(InvocationTerminal::Settled {
                    state: committed_state()?,
                    kind: SettledInvocationKind::Succeeded,
                })
    ));
    let StatusDecision::Retained(status) = decision else {
        return Err("terminal invocation was not retained".into());
    };
    assert_eq!(
        status.status().progress(),
        InvocationProgress::Terminal(InvocationTerminal::Settled {
            state: committed_state()?,
            kind: SettledInvocationKind::Succeeded,
        })
    );
    assert_eq!(status.outcome(), Some(&outcome));
    let event = status
        .committed_event()
        .ok_or("logical commit omitted its recovery event")?;
    assert_eq!(
        InvocationStatusCodec::decode(event.payload())?.progress(),
        InvocationProgress::LogicalCommitted(committed_state()?)
    );
    Ok(())
}

#[cfg(windows)]
#[test]
fn sqlite_path_preserves_unpaired_utf16_surrogates() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let filename = std::ffi::OsString::from_wide(&[
        u16::from(b'w'),
        u16::from(b't'),
        u16::from(b'f'),
        0xd800,
        u16::from(b'.'),
        u16::from(b's'),
        u16::from(b'q'),
        u16::from(b'l'),
        u16::from(b'i'),
        u16::from(b't'),
        u16::from(b'e'),
    ]);
    let path = directory.path().join(filename);
    let fixture = IdentityFixture::new()?;

    let ledger = open_registered(&path, &fixture)?;
    drop(ledger);

    let mut reopened = DurableInvocationLedger::open(&path)?;
    assert_eq!(
        reopened.register_namespace(fixture.namespace, fixture.principal)?,
        NamespaceRegistration::Existing
    );
    Ok(())
}

#[test]
fn restart_never_blindly_replays_an_ambiguous_effect() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("recovery.sqlite");
    let fixture = IdentityFixture::new()?;
    let mut ledger = open_registered(&path, &fixture)?;
    lease_one(&mut ledger, &fixture)?;
    lease_one(&mut ledger, &fixture)?;
    let logical_id = fixture.id(1)?;
    let dispatched_id = fixture.id(2)?;
    let InvocationCommitDecision::Committed(_) = commit(&mut ledger, &fixture, logical_id, 2, 2)?
    else {
        return Err("logical invocation was not committed".into());
    };
    assert!(matches!(
        ledger.cancel_invocation(
            fixture.principal,
            logical_id,
            LedgerTimestamp::from_unix_millis(3)?,
        )?,
        CancelInvocationReply::TooLate(status)
            if status.progress() == InvocationProgress::LogicalCommitted(committed_state()?)
    ));

    let InvocationCommitDecision::Committed(_) =
        commit(&mut ledger, &fixture, dispatched_id, 3, 2)?
    else {
        return Err("dispatched invocation was not committed".into());
    };
    assert_eq!(
        ledger.mark_effect_dispatched(dispatched_id, LedgerTimestamp::from_unix_millis(3)?)?,
        TransitionDecision::Applied
    );
    drop(ledger);

    let mut reopened = DurableInvocationLedger::open(&path)?;
    let report = reopened.recover(LedgerTimestamp::from_unix_millis(4)?)?;
    assert!(report.restarted_before_commit.is_empty());
    assert_eq!(report.indeterminate, [dispatched_id]);
    assert_eq!(report.reconcile.len(), 1);
    assert_eq!(report.reconcile[0].invocation_id, logical_id);
    assert_eq!(report.reconcile[0].state, committed_state()?);
    assert_eq!(report.reconcile[0].dispatch, DispatchState::NotStarted);
    assert_eq!(
        InvocationStatusCodec::decode(report.reconcile[0].committed_event.payload())?.progress(),
        InvocationProgress::LogicalCommitted(committed_state()?)
    );
    assert!(matches!(
        reopened.status(fixture.principal, dispatched_id)?,
        StatusDecision::Retained(status)
            if status.status().progress()
                == InvocationProgress::Terminal(InvocationTerminal::Settled {
                    state: committed_state()?,
                    kind: SettledInvocationKind::Indeterminate,
                })
                && status.committed_event().is_some()
    ));
    Ok(())
}

fn assert_compacted_invocation_is_expired_and_scoped(
    ledger: &mut DurableInvocationLedger,
    fixture: &IdentityFixture,
    id: InvocationId,
    at: i64,
) -> Result<(), Box<dyn Error>> {
    assert_eq!(
        ledger.status(fixture.principal, id)?,
        StatusDecision::InvocationExpired
    );
    assert_eq!(
        ledger.status(fixture.principal, id)?.into_reply(),
        InvocationStatusReply::Unavailable(InvocationUnavailable::Expired)
    );
    assert_eq!(
        ledger.cancel_invocation(
            fixture.principal,
            id,
            LedgerTimestamp::from_unix_millis(at)?,
        )?,
        CancelInvocationReply::Unavailable(InvocationUnavailable::Expired)
    );
    assert_eq!(
        ledger.cancel_invocation(
            fixture.other_principal,
            id,
            LedgerTimestamp::from_unix_millis(at)?,
        )?,
        CancelInvocationReply::Unavailable(InvocationUnavailable::Forbidden)
    );
    assert_eq!(
        ledger.cancel_invocation(
            fixture.principal,
            fixture.id(2)?,
            LedgerTimestamp::from_unix_millis(at)?,
        )?,
        CancelInvocationReply::Unavailable(InvocationUnavailable::UnknownInvocation)
    );
    assert_eq!(
        ledger.cancel_invocation(
            fixture.principal,
            InvocationId::new(
                InvocationNamespaceId::new([8; 16])?,
                InvocationSequence::try_from(1)?,
            ),
            LedgerTimestamp::from_unix_millis(at)?,
        )?,
        CancelInvocationReply::Unavailable(InvocationUnavailable::UnknownNamespace)
    );
    Ok(())
}

#[test]
fn compaction_advances_expiry_only_after_the_retention_floor() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("compaction.sqlite");
    let fixture = IdentityFixture::new()?;
    let id = fixture.id(1)?;
    let mut ledger = open_registered(&path, &fixture)?;
    lease_one(&mut ledger, &fixture)?;
    let InvocationCommitDecision::Committed(_) = commit(&mut ledger, &fixture, id, 1, 1)? else {
        return Err("invocation was not committed".into());
    };
    assert_eq!(
        ledger.record_terminal(
            id,
            TerminalRecord {
                kind: SettledInvocationKind::Succeeded,
                outcome: OutcomeDocument::new(NonZeroU16::MIN, [1])?,
                recorded_at: LedgerTimestamp::from_unix_millis(2)?,
            },
        )?,
        TransitionDecision::Applied
    );
    assert!(matches!(
        ledger.cancel_invocation(
            fixture.principal,
            id,
            LedgerTimestamp::from_unix_millis(3)?,
        )?,
        CancelInvocationReply::AlreadyTerminal(status)
            if status.progress()
                == InvocationProgress::Terminal(InvocationTerminal::Settled {
                    state: committed_state()?,
                    kind: SettledInvocationKind::Succeeded,
                })
    ));

    let retention = TerminalRetention::new(MINIMUM_TERMINAL_RETENTION)?;
    assert!(matches!(
        ledger.compact(
            fixture.namespace,
            fixture.principal,
            id.sequence(),
            LedgerTimestamp::from_unix_millis(2)?,
            retention,
        )?,
        CompactionDecision::Blocked { .. }
    ));
    let retained_until = i64::try_from(MINIMUM_TERMINAL_RETENTION.as_millis())? + 2;
    assert!(matches!(
        ledger.compact(
            fixture.namespace,
            fixture.principal,
            id.sequence(),
            LedgerTimestamp::from_unix_millis(retained_until)?,
            retention,
        )?,
        CompactionDecision::Compacted {
            removed: 1,
            minimum_accepted,
        } if minimum_accepted == InvocationSequence::try_from(2)?
    ));
    assert_compacted_invocation_is_expired_and_scoped(&mut ledger, &fixture, id, retained_until)?;
    assert_eq!(
        commit(&mut ledger, &fixture, id, 1, retained_until)?,
        InvocationCommitDecision::InvocationExpired
    );
    Ok(())
}
