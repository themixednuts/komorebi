use drizzle::core::expr::and;
use drizzle::core::expr::eq;
use drizzle::core::expr::lt;
use drizzle::error::DrizzleError;
use drizzle::sqlite::connection::SQLiteTransactionType;
use komorebi_protocol::InvocationId;
use komorebi_protocol::InvocationNamespaceId;
use komorebi_protocol::PrincipalId;

use super::DurableInvocationLedger;
use super::LedgerError;
use super::is_missing;
use crate::model::CompactionBlock;
use crate::model::CompactionDecision;
use crate::model::TerminalRetention;
use crate::schema::CompactionCandidate;
use crate::schema::SelectInvocationLeases;
use crate::schema::StoredPhase;
use crate::schema::UpdateInvocationLeases;
use crate::storage::StoredNamespaceId;
use crate::storage::StoredPrincipalId;
use crate::storage::StoredSequence;

impl DurableInvocationLedger {
    /// Compacts one continuous namespace prefix while preserving the 24-hour
    /// terminal-retention floor.
    ///
    /// # Errors
    ///
    /// Returns [`LedgerError`] if the typed durable transaction fails or the
    /// retention duration cannot be represented by the ledger clock.
    pub fn compact(
        &mut self,
        namespace: InvocationNamespaceId,
        principal: PrincipalId,
        through: komorebi_protocol::InvocationSequence,
        now: crate::model::LedgerTimestamp,
        retention: TerminalRetention,
    ) -> Result<CompactionDecision, LedgerError> {
        let retention_ms = i64::try_from(retention.duration().as_millis())
            .map_err(|_| LedgerError::RetentionOverflow)?;
        let cutoff_ms = now.as_unix_millis().saturating_sub(retention_ms);
        let namespaces = self.schema.leases;
        let records = self.schema.invocations;
        let namespace_key = StoredNamespaceId(namespace);
        let principal_key = StoredPrincipalId(principal);

        self.db
            .transaction(SQLiteTransactionType::Immediate, |transaction| {
                let namespace_row: SelectInvocationLeases = match transaction
                    .select(())
                    .from(namespaces)
                    .r#where(eq(namespaces.namespace_id, namespace_key))
                    .get()
                {
                    Ok(namespace_row) => namespace_row,
                    Err(error) if is_missing(&error) => {
                        return Ok(CompactionDecision::UnknownNamespace);
                    }
                    Err(error) => return Err(error),
                };
                if namespace_row.principal != principal_key {
                    return Ok(CompactionDecision::PrincipalConflict);
                }

                let Ok(next_minimum) = through.next() else {
                    return Ok(CompactionDecision::SequenceExhausted);
                };
                let next_minimum = StoredSequence(next_minimum);
                if next_minimum <= namespace_row.minimum_accepted {
                    return Ok(CompactionDecision::AlreadyCompacted);
                }
                if next_minimum > namespace_row.next_sequence {
                    return Ok(CompactionDecision::BeyondLeasedRange);
                }

                let mut rows: Vec<CompactionCandidate> = transaction
                    .select(CompactionCandidate::Select)
                    .from(records)
                    .r#where(and(
                        eq(records.namespace, namespace_key),
                        lt(records.sequence, next_minimum),
                    ))
                    .all()?;
                rows.sort_unstable_by_key(|row| row.sequence);

                if let Some((invocation_id, reason)) = first_compaction_block(&rows, cutoff_ms) {
                    return Ok(CompactionDecision::Blocked {
                        invocation_id,
                        reason,
                    });
                }

                let removed = transaction
                    .delete(records)
                    .r#where(and(
                        eq(records.namespace, namespace_key),
                        lt(records.sequence, next_minimum),
                    ))
                    .execute()?;
                if removed != rows.len() {
                    return Err(DrizzleError::Other(
                        "compaction delete count changed inside a single-writer transaction".into(),
                    ));
                }
                let record_count = remaining_record_count(namespace_row.record_count, removed)?;
                let updated = transaction
                    .update(namespaces)
                    .set(
                        UpdateInvocationLeases::default()
                            .with_minimum_accepted(next_minimum)
                            .with_record_count(record_count),
                    )
                    .r#where(and(
                        eq(namespaces.namespace_id, namespace_key),
                        eq(namespaces.principal, principal_key),
                    ))
                    .execute()?;
                if updated != 1 {
                    return Err(DrizzleError::Other(
                        "compaction lost namespace ownership".into(),
                    ));
                }

                let removed = u32::try_from(removed).map_err(|_| {
                    DrizzleError::ConversionError("compaction count does not fit u32".into())
                })?;
                Ok(CompactionDecision::Compacted {
                    removed,
                    minimum_accepted: next_minimum.0,
                })
            })
            .map_err(Into::into)
    }
}

fn first_compaction_block(
    rows: &[CompactionCandidate],
    cutoff_ms: i64,
) -> Option<(InvocationId, CompactionBlock)> {
    rows.iter().find_map(|row| {
        let invocation_id = InvocationId::new(row.namespace.0, row.sequence.0);
        if row.phase != StoredPhase::Terminal {
            Some((
                invocation_id,
                CompactionBlock::NonTerminal(row.phase.into()),
            ))
        } else if row
            .terminal_at_ms
            .is_none_or(|terminal_at| terminal_at > cutoff_ms)
        {
            Some((invocation_id, CompactionBlock::RetentionFloor))
        } else {
            None
        }
    })
}

fn remaining_record_count(current: i64, removed: usize) -> Result<i64, DrizzleError> {
    let removed = i64::try_from(removed)
        .map_err(|_| DrizzleError::ConversionError("compaction count does not fit i64".into()))?;
    current.checked_sub(removed).ok_or_else(|| {
        DrizzleError::ConversionError("namespace record count underflow during compaction".into())
    })
}
