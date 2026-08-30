use std::error::Error;
use std::num::NonZeroU16;
use std::num::NonZeroU32;
use std::num::NonZeroU64;
#[cfg(windows)]
use std::os::windows::ffi::OsStringExt;

use komorebi_command_store::ActionParameterDocument;
use komorebi_command_store::CommittedEventDocument;
use komorebi_command_store::CommittedRevision;
use komorebi_command_store::CompactionDecision;
use komorebi_command_store::DispatchState;
use komorebi_command_store::DurableInvocationLedger;
use komorebi_command_store::DurablePhase;
use komorebi_command_store::LeaseDecision;
use komorebi_command_store::LeaseRequest;
use komorebi_command_store::LedgerTimestamp;
use komorebi_command_store::MINIMUM_TERMINAL_RETENTION;
use komorebi_command_store::NamespaceRegistration;
use komorebi_command_store::OutcomeDocument;
use komorebi_command_store::RecoveryPolicy;
use komorebi_command_store::ReservationDecision;
use komorebi_command_store::ReservationRequest;
use komorebi_command_store::StatusDecision;
use komorebi_command_store::TerminalKind;
use komorebi_command_store::TerminalRecord;
use komorebi_command_store::TerminalRetention;
use komorebi_command_store::TransitionDecision;
use komorebi_protocol::InvocationDigest;
use komorebi_protocol::InvocationId;
use komorebi_protocol::InvocationNamespaceId;
use komorebi_protocol::InvocationSequence;
use komorebi_protocol::PrincipalId;

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

fn document(payload: u8) -> Result<ActionParameterDocument, Box<dyn Error>> {
    Ok(ActionParameterDocument::new(NonZeroU16::MIN, [payload])?)
}

fn digest(value: u8) -> Result<InvocationDigest, Box<dyn Error>> {
    Ok(InvocationDigest::new([value; 32])?)
}

fn reserve(
    ledger: &mut DurableInvocationLedger,
    fixture: &IdentityFixture,
    id: InvocationId,
    digest: InvocationDigest,
    at: i64,
) -> Result<ReservationDecision, Box<dyn Error>> {
    Ok(ledger.reserve(ReservationRequest {
        principal: fixture.principal,
        invocation_id: id,
        digest,
        parameters: document(9)?,
        reserved_at: LedgerTimestamp::from_unix_millis(at)?,
    })?)
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
fn terminal_status_survives_reopen_on_a_unicode_windows_path() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("台帳-💮.sqlite");
    let fixture = IdentityFixture::new()?;
    let id = fixture.id(1)?;
    let invocation_digest = digest(7)?;
    let mut ledger = open_registered(&path, &fixture)?;
    lease_one(&mut ledger, &fixture)?;

    let reservation = match reserve(&mut ledger, &fixture, id, invocation_digest, 1)? {
        ReservationDecision::Reserved(reservation) => reservation,
        other => return Err(format!("expected reservation, got {other:?}").into()),
    };
    assert!(matches!(
        reserve(&mut ledger, &fixture, id, invocation_digest, 1)?,
        ReservationDecision::Retained(_)
    ));
    assert_eq!(
        reserve(&mut ledger, &fixture, id, digest(8)?, 1)?,
        ReservationDecision::IdempotencyConflict
    );
    assert_eq!(
        ledger.reserve(ReservationRequest {
            principal: fixture.other_principal,
            invocation_id: id,
            digest: invocation_digest,
            parameters: document(9)?,
            reserved_at: LedgerTimestamp::from_unix_millis(1)?,
        })?,
        ReservationDecision::IdempotencyConflict
    );

    assert_eq!(
        ledger.commit_logical(
            reservation,
            CommittedRevision::new(NonZeroU64::MIN),
            RecoveryPolicy::ObserveAndConverge,
            LedgerTimestamp::from_unix_millis(2)?,
        )?,
        TransitionDecision::Applied
    );
    assert_eq!(
        ledger.mark_effect_dispatched(id, LedgerTimestamp::from_unix_millis(3)?)?,
        TransitionDecision::Applied
    );
    let outcome = OutcomeDocument::new(NonZeroU16::MIN, [4])?;
    let event = CommittedEventDocument::new(NonZeroU16::MIN, [5])?;
    assert_eq!(
        ledger.record_terminal(
            id,
            TerminalRecord {
                kind: TerminalKind::Succeeded,
                outcome: outcome.clone(),
                committed_event: event.clone(),
                recorded_at: LedgerTimestamp::from_unix_millis(4)?,
            },
        )?,
        TransitionDecision::Applied
    );
    drop(ledger);

    let reopened = DurableInvocationLedger::open(&path)?;
    let StatusDecision::Retained(status) = reopened.status(fixture.principal, id)? else {
        return Err("terminal invocation was not retained".into());
    };
    assert_eq!(status.phase, DurablePhase::Terminal);
    assert_eq!(status.terminal_kind, Some(TerminalKind::Succeeded));
    assert_eq!(status.outcome, Some(outcome));
    assert_eq!(status.committed_event, Some(event));
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
    lease_one(&mut ledger, &fixture)?;

    let reserved_id = fixture.id(1)?;
    let logical_id = fixture.id(2)?;
    let dispatched_id = fixture.id(3)?;
    let _reserved = reserve(&mut ledger, &fixture, reserved_id, digest(1)?, 1)?;

    let ReservationDecision::Reserved(logical) =
        reserve(&mut ledger, &fixture, logical_id, digest(2)?, 1)?
    else {
        return Err("logical invocation was not reserved".into());
    };
    assert_eq!(
        ledger.commit_logical(
            logical,
            CommittedRevision::new(NonZeroU64::MIN),
            RecoveryPolicy::NeverReplay,
            LedgerTimestamp::from_unix_millis(2)?,
        )?,
        TransitionDecision::Applied
    );
    assert_eq!(
        ledger.cancel_reserved(logical_id, LedgerTimestamp::from_unix_millis(3)?)?,
        TransitionDecision::WrongPhase(DurablePhase::LogicalCommitted)
    );

    let ReservationDecision::Reserved(dispatched) =
        reserve(&mut ledger, &fixture, dispatched_id, digest(3)?, 1)?
    else {
        return Err("dispatched invocation was not reserved".into());
    };
    assert_eq!(
        ledger.commit_logical(
            dispatched,
            CommittedRevision::new(NonZeroU64::MIN),
            RecoveryPolicy::NeverReplay,
            LedgerTimestamp::from_unix_millis(2)?,
        )?,
        TransitionDecision::Applied
    );
    assert_eq!(
        ledger.mark_effect_dispatched(dispatched_id, LedgerTimestamp::from_unix_millis(3)?)?,
        TransitionDecision::Applied
    );
    drop(ledger);

    let mut reopened = DurableInvocationLedger::open(&path)?;
    let report = reopened.recover(LedgerTimestamp::from_unix_millis(4)?)?;
    assert_eq!(report.restarted_before_commit, [reserved_id]);
    assert_eq!(report.indeterminate, [dispatched_id]);
    assert_eq!(report.reconcile.len(), 1);
    assert_eq!(report.reconcile[0].invocation_id, logical_id);
    assert_eq!(report.reconcile[0].dispatch, DispatchState::NotStarted);
    assert!(matches!(
        reopened.status(fixture.principal, reserved_id)?,
        StatusDecision::Retained(status)
            if status.terminal_kind == Some(TerminalKind::RestartedBeforeCommit)
    ));
    assert!(matches!(
        reopened.status(fixture.principal, dispatched_id)?,
        StatusDecision::Retained(status)
            if status.terminal_kind == Some(TerminalKind::Indeterminate)
    ));
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
    let ReservationDecision::Reserved(_) = reserve(&mut ledger, &fixture, id, digest(1)?, 1)?
    else {
        return Err("invocation was not reserved".into());
    };
    assert_eq!(
        ledger.cancel_reserved(id, LedgerTimestamp::from_unix_millis(2)?)?,
        TransitionDecision::Applied
    );

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
    assert_eq!(
        ledger.status(fixture.principal, id)?,
        StatusDecision::InvocationExpired
    );
    assert_eq!(
        reserve(&mut ledger, &fixture, id, digest(1)?, retained_until)?,
        ReservationDecision::InvocationExpired
    );
    Ok(())
}
