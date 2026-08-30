use drizzle::core::expr::and;
use drizzle::core::expr::eq;
use drizzle::core::expr::ne;
use drizzle::error::DrizzleError;
use drizzle::sqlite::connection::SQLiteTransactionType;
use komorebi_protocol::ActionInvocation;
use komorebi_protocol::ActionInvocationCodec;
use komorebi_protocol::InvocationId;
use komorebi_protocol::InvocationProgress;
use komorebi_protocol::InvocationStatus;
use komorebi_protocol::InvocationStatusCodec;
use komorebi_protocol::PrincipalId;

use super::DurableInvocationLedger;
use super::LedgerError;
use super::is_missing;
use super::status_from_row;
use crate::document::CommittedEventDocument;
use crate::document::InvocationDocument;
use crate::model::CommittedInvocation;
use crate::model::InvocationCommit;
use crate::model::InvocationCommitDecision;
use crate::model::InvocationInspection;
use crate::model::MAX_LIVE_RECORDS_PER_NAMESPACE;
use crate::model::StatusDecision;
use crate::model::TerminalRecord;
use crate::model::TransitionDecision;
use crate::schema::InsertInvocations;
use crate::schema::InvocationPhase;
use crate::schema::InvocationSnapshot;
use crate::schema::SelectInvocationLeases;
use crate::schema::StoredPhase;
use crate::schema::StoredRecoveryPolicy;
use crate::schema::StoredTerminalKind;
use crate::schema::UpdateInvocationLeases;
use crate::schema::UpdateInvocations;
use crate::storage::StoredDigest;
use crate::storage::StoredNamespaceId;
use crate::storage::StoredPrincipalId;
use crate::storage::StoredSequence;
use crate::storage::StoredStateStamp;

impl DurableInvocationLedger {
    /// Inspects idempotency before manager admission without reserving a new
    /// invocation identity.
    ///
    /// This lets the serialized manager owner return an existing result before
    /// reevaluating stale catalog/state preconditions. A vacant result is only
    /// a snapshot; callers must still handle every [`InvocationCommitDecision`]
    /// when they atomically commit after successful preparation.
    ///
    /// # Errors
    ///
    /// Returns [`LedgerError`] when canonical encoding or the typed durable
    /// query fails.
    pub fn inspect_invocation(
        &self,
        principal: PrincipalId,
        invocation: &ActionInvocation,
    ) -> Result<InvocationInspection, LedgerError> {
        let digest = ActionInvocationCodec::canonicalize(invocation)?.digest();
        Ok(match self.status(principal, invocation.invocation_id())? {
            StatusDecision::Retained(record) if record.status().digest() == digest => {
                InvocationInspection::Retained(record)
            }
            StatusDecision::Retained(_) | StatusDecision::PrincipalConflict => {
                InvocationInspection::IdempotencyConflict
            }
            StatusDecision::InvocationExpired => InvocationInspection::InvocationExpired,
            StatusDecision::UnknownInvocation => InvocationInspection::Vacant,
            StatusDecision::UnknownNamespace => InvocationInspection::UnknownNamespace,
        })
    }

    /// Atomically claims an authenticated invocation and publishes its logical
    /// manager revision before native effect dispatch can begin.
    ///
    /// Canonical invocation bytes, their digest, the logical state, and the
    /// exact committed event are written in one immediate transaction. A
    /// failed operation therefore cannot consume an invocation identity
    /// without its logical commit.
    ///
    /// # Errors
    ///
    /// Returns [`LedgerError`] when the logical state is not the invocation's
    /// immediate successor, canonical encoding fails, or the typed durable
    /// transaction fails.
    pub fn commit_invocation(
        &mut self,
        principal: PrincipalId,
        invocation: &ActionInvocation,
        state: komorebi_protocol::StateStamp,
        recovery_policy: crate::model::RecoveryPolicy,
        committed_at: crate::model::LedgerTimestamp,
    ) -> Result<InvocationCommitDecision, LedgerError> {
        if invocation.expected_state().next().ok() != Some(state) {
            return Err(LedgerError::CommitStateMismatch);
        }

        let canonical = ActionInvocationCodec::canonicalize(invocation)?;
        let digest = canonical.digest();
        let committed_status = InvocationStatus::new(
            invocation.invocation_id(),
            digest,
            InvocationProgress::LogicalCommitted(state),
        );
        let committed_event = CommittedEventDocument::new(
            std::num::NonZeroU16::MIN,
            InvocationStatusCodec::encode(committed_status)?,
        )?;
        self.claim(InvocationCommit {
            principal,
            invocation_id: invocation.invocation_id(),
            digest,
            invocation: InvocationDocument::new(std::num::NonZeroU16::MIN, canonical.into_bytes())?,
            state,
            recovery_policy,
            committed_event,
            committed_at,
        })
    }

    fn claim(&mut self, commit: InvocationCommit) -> Result<InvocationCommitDecision, LedgerError> {
        let namespaces = self.schema.leases;
        let records = self.schema.invocations;
        let id = commit.invocation_id;
        let namespace = StoredNamespaceId(id.namespace());
        let sequence = StoredSequence(id.sequence());
        let principal = StoredPrincipalId(commit.principal);
        let digest = StoredDigest(commit.digest);

        self.db
            .transaction(SQLiteTransactionType::Immediate, |transaction| {
                let existing: Result<InvocationSnapshot, DrizzleError> = transaction
                    .select(InvocationSnapshot::Select)
                    .from(records)
                    .r#where(invocation_predicate!(records, id))
                    .get();
                match existing {
                    Ok(existing)
                        if existing.principal == principal && existing.digest == digest =>
                    {
                        return Ok(InvocationCommitDecision::Retained(status_from_row(
                            existing,
                        )?));
                    }
                    Ok(_) => return Ok(InvocationCommitDecision::IdempotencyConflict),
                    Err(error) if is_missing(&error) => {}
                    Err(error) => return Err(error),
                }

                let namespace_row: SelectInvocationLeases = match transaction
                    .select(())
                    .from(namespaces)
                    .r#where(eq(namespaces.namespace_id, namespace))
                    .get()
                {
                    Ok(namespace) => namespace,
                    Err(error) if is_missing(&error) => {
                        return Ok(InvocationCommitDecision::UnknownNamespace);
                    }
                    Err(error) => return Err(error),
                };
                if namespace_row.principal != principal {
                    return Ok(InvocationCommitDecision::IdempotencyConflict);
                }
                if sequence < namespace_row.minimum_accepted {
                    return Ok(InvocationCommitDecision::InvocationExpired);
                }
                if sequence >= namespace_row.next_sequence {
                    return Ok(InvocationCommitDecision::InvocationNotLeased);
                }
                if namespace_row.record_count >= MAX_LIVE_RECORDS_PER_NAMESPACE {
                    return Ok(InvocationCommitDecision::CapacityFull);
                }

                transaction
                    .insert(records)
                    .values([InsertInvocations::new(
                        namespace,
                        sequence,
                        principal,
                        digest,
                        commit.invocation,
                        StoredPhase::LogicalCommitted,
                        commit.committed_at.as_unix_millis(),
                    )
                    .with_recovery_policy(StoredRecoveryPolicy::from(commit.recovery_policy))
                    .with_state_stamp(StoredStateStamp(commit.state))
                    .with_committed_event(commit.committed_event)
                    .with_logical_committed_at_ms(commit.committed_at.as_unix_millis())])
                    .execute()?;
                let updated = transaction
                    .update(namespaces)
                    .set(
                        UpdateInvocationLeases::default()
                            .with_record_count(namespace_row.record_count + 1),
                    )
                    .r#where(and(
                        eq(namespaces.namespace_id, namespace),
                        eq(namespaces.principal, principal),
                    ))
                    .execute()?;
                if updated != 1 {
                    return Err(DrizzleError::Other(
                        "invocation count update lost namespace ownership".into(),
                    ));
                }

                Ok(InvocationCommitDecision::Committed(
                    CommittedInvocation::new(id, commit.digest),
                ))
            })
            .map_err(Into::into)
    }

    /// Records that native dispatch began. Cancellation after this point never
    /// causes an automatic retry.
    ///
    /// # Errors
    ///
    /// Returns [`LedgerError`] if the typed durable transition fails.
    pub fn mark_effect_dispatched(
        &mut self,
        invocation_id: InvocationId,
        dispatched_at: crate::model::LedgerTimestamp,
    ) -> Result<TransitionDecision, LedgerError> {
        let update = UpdateInvocations::default()
            .with_phase(StoredPhase::EffectDispatched)
            .with_effect_dispatched_at_ms(dispatched_at.as_unix_millis());
        self.transition_from(invocation_id, StoredPhase::LogicalCommitted, update)
    }

    /// Records the terminal outcome after logical state and its event are durable.
    ///
    /// # Errors
    ///
    /// Returns [`LedgerError`] if the typed durable transaction fails.
    pub fn record_terminal(
        &mut self,
        invocation_id: InvocationId,
        terminal: TerminalRecord,
    ) -> Result<TransitionDecision, LedgerError> {
        let records = self.schema.invocations;
        let terminal_kind = StoredTerminalKind::from(terminal.kind);
        self.db
            .transaction(SQLiteTransactionType::Immediate, |transaction| {
                let current: InvocationPhase = match transaction
                    .select(InvocationPhase::Select)
                    .from(records)
                    .r#where(invocation_predicate!(records, invocation_id))
                    .get()
                {
                    Ok(current) => current,
                    Err(error) if is_missing(&error) => {
                        return Ok(TransitionDecision::UnknownInvocation);
                    }
                    Err(error) => return Err(error),
                };
                if !matches!(
                    current.phase,
                    StoredPhase::LogicalCommitted | StoredPhase::EffectDispatched
                ) {
                    return Ok(TransitionDecision::WrongPhase(current.phase.into()));
                }

                let updated = transaction
                    .update(records)
                    .set(
                        UpdateInvocations::default()
                            .with_phase(StoredPhase::Terminal)
                            .with_terminal_kind(terminal_kind)
                            .with_outcome(terminal.outcome)
                            .with_terminal_at_ms(terminal.recorded_at.as_unix_millis()),
                    )
                    .r#where(and(
                        invocation_predicate!(records, invocation_id),
                        ne(records.phase, StoredPhase::Terminal),
                    ))
                    .execute()?;
                if updated == 1 {
                    Ok(TransitionDecision::Applied)
                } else {
                    Err(DrizzleError::Other(
                        "terminal transition lost its phase guard".into(),
                    ))
                }
            })
            .map_err(Into::into)
    }

    /// Returns a principal-scoped retained, expired, or unknown status.
    ///
    /// # Errors
    ///
    /// Returns [`LedgerError`] if the typed durable query fails.
    pub fn status(
        &self,
        principal: PrincipalId,
        invocation_id: InvocationId,
    ) -> Result<StatusDecision, LedgerError> {
        let records = self.schema.invocations;
        let record: Result<InvocationSnapshot, DrizzleError> = self
            .db
            .select(InvocationSnapshot::Select)
            .from(records)
            .r#where(invocation_predicate!(records, invocation_id))
            .get();
        match record {
            Ok(row) if row.principal.0 == principal => {
                Ok(StatusDecision::Retained(status_from_row(row)?))
            }
            Ok(_) => Ok(StatusDecision::PrincipalConflict),
            Err(error) if is_missing(&error) => {
                let namespaces = self.schema.leases;
                let namespace: SelectInvocationLeases = match self
                    .db
                    .select(())
                    .from(namespaces)
                    .r#where(eq(
                        namespaces.namespace_id,
                        StoredNamespaceId(invocation_id.namespace()),
                    ))
                    .get()
                {
                    Ok(namespace) => namespace,
                    Err(error) if is_missing(&error) => {
                        return Ok(StatusDecision::UnknownNamespace);
                    }
                    Err(error) => return Err(error.into()),
                };
                if namespace.principal.0 != principal {
                    Ok(StatusDecision::PrincipalConflict)
                } else if invocation_id.sequence() < namespace.minimum_accepted.0 {
                    Ok(StatusDecision::InvocationExpired)
                } else {
                    Ok(StatusDecision::UnknownInvocation)
                }
            }
            Err(error) => Err(error.into()),
        }
    }

    fn transition_from(
        &mut self,
        invocation_id: InvocationId,
        expected: StoredPhase,
        update: UpdateInvocations<'_, drizzle::core::NonEmpty>,
    ) -> Result<TransitionDecision, LedgerError> {
        let records = self.schema.invocations;
        self.db
            .transaction(SQLiteTransactionType::Immediate, |transaction| {
                let updated = transaction
                    .update(records)
                    .set(update)
                    .r#where(and(
                        invocation_predicate!(records, invocation_id),
                        eq(records.phase, expected),
                    ))
                    .execute()?;
                if updated == 1 {
                    return Ok(TransitionDecision::Applied);
                }

                let current: Result<InvocationPhase, DrizzleError> = transaction
                    .select(InvocationPhase::Select)
                    .from(records)
                    .r#where(invocation_predicate!(records, invocation_id))
                    .get();
                match current {
                    Ok(current) => Ok(TransitionDecision::WrongPhase(current.phase.into())),
                    Err(error) if is_missing(&error) => Ok(TransitionDecision::UnknownInvocation),
                    Err(error) => Err(error),
                }
            })
            .map_err(Into::into)
    }
}
