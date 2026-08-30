use drizzle::core::expr::ne;
use drizzle::error::DrizzleError;
use drizzle::sqlite::connection::SQLiteTransactionType;
use komorebi_protocol::InvocationId;

use super::DurableInvocationLedger;
use super::LedgerError;
use crate::model::DispatchState;
use crate::model::RecoveryInvocation;
use crate::model::RecoveryReport;
use crate::schema::RecoveryCandidate;
use crate::schema::StoredPhase;
use crate::schema::StoredRecoveryPolicy;
use crate::schema::StoredTerminalKind;

impl DurableInvocationLedger {
    /// Converts crash-interrupted rows into explicit retained classifications.
    ///
    /// # Errors
    ///
    /// Returns [`LedgerError`] if recovery cannot read or atomically classify
    /// every interrupted invocation.
    pub fn recover(
        &mut self,
        recovered_at: crate::model::LedgerTimestamp,
    ) -> Result<RecoveryReport, LedgerError> {
        let records = self.schema.records;
        self.db
            .transaction(SQLiteTransactionType::Immediate, |transaction| {
                let rows: Vec<RecoveryCandidate> = transaction
                    .select(RecoveryCandidate::Select)
                    .from(records)
                    .r#where(ne(records.phase, StoredPhase::Terminal))
                    .all()?;
                let mut report = RecoveryReport {
                    reconcile: Vec::new(),
                    restarted_before_commit: Vec::new(),
                    indeterminate: Vec::new(),
                };

                for row in rows {
                    let id = InvocationId::new(row.namespace.0, row.sequence.0);
                    match row.phase {
                        StoredPhase::Reserved => {
                            mark_system_terminal!(
                                transaction,
                                records,
                                id,
                                StoredTerminalKind::RestartedBeforeCommit,
                                recovered_at.as_unix_millis(),
                            )?;
                            report.restarted_before_commit.push(id);
                        }
                        StoredPhase::LogicalCommitted | StoredPhase::EffectDispatched => {
                            let revision = row.logical_revision.ok_or_else(|| {
                                DrizzleError::ConversionError(
                                    "committed invocation is missing its revision".into(),
                                )
                            })?;
                            let policy = row.recovery_policy.ok_or_else(|| {
                                DrizzleError::ConversionError(
                                    "committed invocation is missing its recovery policy".into(),
                                )
                            })?;

                            if row.phase == StoredPhase::EffectDispatched
                                && policy == StoredRecoveryPolicy::NeverReplay
                            {
                                mark_system_terminal!(
                                    transaction,
                                    records,
                                    id,
                                    StoredTerminalKind::Indeterminate,
                                    recovered_at.as_unix_millis(),
                                )?;
                                report.indeterminate.push(id);
                            } else {
                                report.reconcile.push(RecoveryInvocation {
                                    invocation_id: id,
                                    revision: revision.0,
                                    policy: policy.into(),
                                    dispatch: if row.phase == StoredPhase::EffectDispatched {
                                        DispatchState::MayHaveOccurred
                                    } else {
                                        DispatchState::NotStarted
                                    },
                                    invocation: row.invocation,
                                });
                            }
                        }
                        StoredPhase::Terminal => {}
                    }
                }

                Ok(report)
            })
            .map_err(Into::into)
    }
}
