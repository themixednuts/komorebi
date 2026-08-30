use drizzle::core::expr::and;
use drizzle::core::expr::eq;
use drizzle::core::expr::ne;
use drizzle::error::DrizzleError;
use drizzle::sqlite::connection::SQLiteTransactionType;
use komorebi_protocol::InvocationId;
use komorebi_protocol::PrincipalId;

use super::DurableInvocationLedger;
use super::LedgerError;
use super::is_missing;
use super::status_from_row;
use crate::model::MAX_LIVE_RECORDS_PER_NAMESPACE;
use crate::model::RecoveryPolicy;
use crate::model::Reservation;
use crate::model::ReservationDecision;
use crate::model::ReservationRequest;
use crate::model::StatusDecision;
use crate::model::TerminalRecord;
use crate::model::TransitionDecision;
use crate::schema::InsertInvocationRecords;
use crate::schema::InvocationPhase;
use crate::schema::InvocationSnapshot;
use crate::schema::SelectInvocationNamespaces;
use crate::schema::StoredPhase;
use crate::schema::StoredRecoveryPolicy;
use crate::schema::StoredTerminalKind;
use crate::schema::UpdateInvocationNamespaces;
use crate::schema::UpdateInvocationRecords;
use crate::storage::CommittedRevision;
use crate::storage::StoredDigest;
use crate::storage::StoredNamespaceId;
use crate::storage::StoredPrincipalId;
use crate::storage::StoredRevision;
use crate::storage::StoredSequence;

impl DurableInvocationLedger {
    /// Durably reserves an invocation before admission.
    ///
    /// # Errors
    ///
    /// Returns [`LedgerError`] if the typed durable transaction fails.
    pub fn reserve(
        &mut self,
        request: ReservationRequest,
    ) -> Result<ReservationDecision, LedgerError> {
        let namespaces = self.schema.namespaces;
        let records = self.schema.records;
        let id = request.invocation_id;
        let namespace = StoredNamespaceId(id.namespace());
        let sequence = StoredSequence(id.sequence());
        let principal = StoredPrincipalId(request.principal);
        let digest = StoredDigest(request.digest);

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
                        return Ok(ReservationDecision::Retained(status_from_row(existing)));
                    }
                    Ok(_) => return Ok(ReservationDecision::IdempotencyConflict),
                    Err(error) if is_missing(&error) => {}
                    Err(error) => return Err(error),
                }

                let namespace_row: SelectInvocationNamespaces = match transaction
                    .select(())
                    .from(namespaces)
                    .r#where(eq(namespaces.namespace, namespace))
                    .get()
                {
                    Ok(namespace) => namespace,
                    Err(error) if is_missing(&error) => {
                        return Ok(ReservationDecision::UnknownNamespace);
                    }
                    Err(error) => return Err(error),
                };
                if namespace_row.principal != principal {
                    return Ok(ReservationDecision::IdempotencyConflict);
                }
                if sequence < namespace_row.minimum_accepted {
                    return Ok(ReservationDecision::InvocationExpired);
                }
                if sequence >= namespace_row.next_sequence {
                    return Ok(ReservationDecision::InvocationNotLeased);
                }
                if namespace_row.record_count >= MAX_LIVE_RECORDS_PER_NAMESPACE {
                    return Ok(ReservationDecision::CapacityFull);
                }

                transaction
                    .insert(records)
                    .values([InsertInvocationRecords::new(
                        namespace,
                        sequence,
                        principal,
                        digest,
                        request.parameters,
                        StoredPhase::Reserved,
                        request.reserved_at.as_unix_millis(),
                    )])
                    .execute()?;
                let updated = transaction
                    .update(namespaces)
                    .set(
                        UpdateInvocationNamespaces::default()
                            .with_record_count(namespace_row.record_count + 1),
                    )
                    .r#where(and(
                        eq(namespaces.namespace, namespace),
                        eq(namespaces.principal, principal),
                    ))
                    .execute()?;
                if updated != 1 {
                    return Err(DrizzleError::Other(
                        "reservation count update lost namespace ownership".into(),
                    ));
                }

                Ok(ReservationDecision::Reserved(Reservation::new(
                    id,
                    request.digest,
                )))
            })
            .map_err(Into::into)
    }

    /// Commits logical manager state before any native effect dispatch.
    ///
    /// # Errors
    ///
    /// Returns [`LedgerError`] if the typed durable transition fails.
    pub fn commit_logical(
        &mut self,
        reservation: Reservation,
        revision: CommittedRevision,
        policy: RecoveryPolicy,
        committed_at: crate::model::LedgerTimestamp,
    ) -> Result<TransitionDecision, LedgerError> {
        let policy = StoredRecoveryPolicy::from(policy);
        let update = UpdateInvocationRecords::default()
            .with_phase(StoredPhase::LogicalCommitted)
            .with_logical_revision(StoredRevision(revision))
            .with_recovery_policy(policy)
            .with_logical_committed_at_ms(committed_at.as_unix_millis());
        self.transition_from(reservation.invocation_id(), StoredPhase::Reserved, update)
    }

    /// Atomically lets cancellation win only while the invocation is reserved.
    ///
    /// # Errors
    ///
    /// Returns [`LedgerError`] if the typed durable transition fails.
    pub fn cancel_reserved(
        &mut self,
        invocation_id: InvocationId,
        cancelled_at: crate::model::LedgerTimestamp,
    ) -> Result<TransitionDecision, LedgerError> {
        let update = UpdateInvocationRecords::default()
            .with_phase(StoredPhase::Terminal)
            .with_terminal_kind(StoredTerminalKind::CancelledBeforeCommit)
            .with_terminal_at_ms(cancelled_at.as_unix_millis());
        self.transition_from(invocation_id, StoredPhase::Reserved, update)
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
        let update = UpdateInvocationRecords::default()
            .with_phase(StoredPhase::EffectDispatched)
            .with_effect_dispatched_at_ms(dispatched_at.as_unix_millis());
        self.transition_from(invocation_id, StoredPhase::LogicalCommitted, update)
    }

    /// Records the terminal outcome and committed event in one transaction.
    ///
    /// # Errors
    ///
    /// Returns [`LedgerError`] if the typed durable transaction fails.
    pub fn record_terminal(
        &mut self,
        invocation_id: InvocationId,
        terminal: TerminalRecord,
    ) -> Result<TransitionDecision, LedgerError> {
        let records = self.schema.records;
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
                        UpdateInvocationRecords::default()
                            .with_phase(StoredPhase::Terminal)
                            .with_terminal_kind(terminal_kind)
                            .with_outcome(terminal.outcome)
                            .with_committed_event(terminal.committed_event)
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
        let records = self.schema.records;
        let record: Result<InvocationSnapshot, DrizzleError> = self
            .db
            .select(InvocationSnapshot::Select)
            .from(records)
            .r#where(invocation_predicate!(records, invocation_id))
            .get();
        match record {
            Ok(row) if row.principal.0 == principal => {
                Ok(StatusDecision::Retained(status_from_row(row)))
            }
            Ok(_) => Ok(StatusDecision::PrincipalConflict),
            Err(error) if is_missing(&error) => {
                let namespaces = self.schema.namespaces;
                let namespace: SelectInvocationNamespaces = match self
                    .db
                    .select(())
                    .from(namespaces)
                    .r#where(eq(
                        namespaces.namespace,
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
        update: UpdateInvocationRecords<'_, drizzle::core::NonEmpty>,
    ) -> Result<TransitionDecision, LedgerError> {
        let records = self.schema.records;
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
