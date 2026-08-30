use drizzle::core::expr::and;
use drizzle::core::expr::eq;
use drizzle::error::DrizzleError;
use drizzle::sqlite::connection::SQLiteTransactionType;
use komorebi_protocol::CancelInvocationReply;
use komorebi_protocol::InvocationId;
use komorebi_protocol::InvocationProgress;
use komorebi_protocol::InvocationStatus;
use komorebi_protocol::InvocationTerminal;
use komorebi_protocol::InvocationUnavailable;
use komorebi_protocol::PrincipalId;

use super::DurableInvocationLedger;
use super::LedgerError;
use super::is_missing;
use super::status_from_row;
use crate::schema::InvocationSnapshot;
use crate::schema::SelectInvocationLeases;
use crate::schema::StoredPhase;
use crate::schema::StoredTerminalKind;
use crate::schema::UpdateInvocations;
use crate::storage::StoredNamespaceId;
use crate::storage::StoredPrincipalId;

impl DurableInvocationLedger {
    /// Atomically lets cancellation win only while the authenticated
    /// principal's invocation remains durably reserved.
    ///
    /// # Errors
    ///
    /// Returns [`LedgerError`] if the typed durable transaction fails.
    pub fn cancel_invocation(
        &mut self,
        principal: PrincipalId,
        invocation_id: InvocationId,
        cancelled_at: crate::model::LedgerTimestamp,
    ) -> Result<CancelInvocationReply, LedgerError> {
        let records = self.schema.invocations;
        let leases = self.schema.leases;
        let principal = StoredPrincipalId(principal);
        self.db
            .transaction(SQLiteTransactionType::Immediate, |transaction| {
                let row: InvocationSnapshot = match transaction
                    .select(InvocationSnapshot::Select)
                    .from(records)
                    .r#where(invocation_predicate!(records, invocation_id))
                    .get()
                {
                    Ok(row) => row,
                    Err(error) if is_missing(&error) => {
                        let lease: SelectInvocationLeases = match transaction
                            .select(())
                            .from(leases)
                            .r#where(eq(
                                leases.namespace_id,
                                StoredNamespaceId(invocation_id.namespace()),
                            ))
                            .get()
                        {
                            Ok(lease) => lease,
                            Err(error) if is_missing(&error) => {
                                return Ok(CancelInvocationReply::Unavailable(
                                    InvocationUnavailable::UnknownNamespace,
                                ));
                            }
                            Err(error) => return Err(error),
                        };
                        let reason = if lease.principal != principal {
                            InvocationUnavailable::Forbidden
                        } else if invocation_id.sequence() < lease.minimum_accepted.0 {
                            InvocationUnavailable::Expired
                        } else {
                            InvocationUnavailable::UnknownInvocation
                        };
                        return Ok(CancelInvocationReply::Unavailable(reason));
                    }
                    Err(error) => return Err(error),
                };
                if row.principal != principal {
                    return Ok(CancelInvocationReply::Unavailable(
                        InvocationUnavailable::Forbidden,
                    ));
                }

                let current = status_from_row(row)?.status();
                match current.progress() {
                    InvocationProgress::Reserved => {
                        let updated = transaction
                            .update(records)
                            .set(
                                UpdateInvocations::default()
                                    .with_phase(StoredPhase::Terminal)
                                    .with_terminal_kind(StoredTerminalKind::CancelledBeforeCommit)
                                    .with_terminal_at_ms(cancelled_at.as_unix_millis()),
                            )
                            .r#where(and(
                                invocation_predicate!(records, invocation_id),
                                and(
                                    eq(records.principal, principal),
                                    eq(records.phase, StoredPhase::Reserved),
                                ),
                            ))
                            .execute()?;
                        if updated != 1 {
                            return Err(DrizzleError::Other(
                                "cancellation lost its durable reservation guard".into(),
                            ));
                        }
                        Ok(CancelInvocationReply::Cancelled(InvocationStatus::new(
                            current.invocation_id(),
                            current.digest(),
                            InvocationProgress::Terminal(InvocationTerminal::CancelledBeforeCommit),
                        )))
                    }
                    InvocationProgress::LogicalCommitted(_)
                    | InvocationProgress::EffectDispatched(_) => {
                        Ok(CancelInvocationReply::TooLate(current))
                    }
                    InvocationProgress::Terminal(_) => {
                        Ok(CancelInvocationReply::AlreadyTerminal(current))
                    }
                }
            })
            .map_err(Into::into)
    }
}
