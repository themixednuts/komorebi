use std::path::Path;

use drizzle::error::DrizzleError;
use drizzle::migrations::Tracking;
use drizzle::sqlite::rusqlite::Drizzle;
use komorebi_protocol::InvocationId;
use thiserror::Error;

use crate::model::DurablePhase;
use crate::model::InvocationStatus;
use crate::model::RecoveryPolicy;
use crate::model::TerminalKind;
use crate::path::configure_durability;
use crate::path::open_sqlite;
use crate::schema::CommandStoreSchema;
use crate::schema::InvocationSnapshot;
use crate::schema::StoredPhase;
use crate::schema::StoredRecoveryPolicy;
use crate::schema::StoredTerminalKind;

type StoreDb = Drizzle<CommandStoreSchema>;

macro_rules! invocation_predicate {
    ($records:expr, $id:expr) => {
        drizzle::core::expr::and(
            drizzle::core::expr::eq(
                $records.namespace,
                $crate::storage::StoredNamespaceId($id.namespace()),
            ),
            drizzle::core::expr::eq(
                $records.sequence,
                $crate::storage::StoredSequence($id.sequence()),
            ),
        )
    };
}

macro_rules! mark_system_terminal {
    ($transaction:expr, $records:expr, $id:expr, $kind:expr, $at_ms:expr $(,)?) => {{
        let updated = $transaction
            .update($records)
            .set(
                $crate::schema::UpdateInvocationRecords::default()
                    .with_phase(StoredPhase::Terminal)
                    .with_terminal_kind($kind)
                    .with_terminal_at_ms($at_ms),
            )
            .r#where(drizzle::core::expr::and(
                invocation_predicate!($records, $id),
                drizzle::core::expr::ne($records.phase, $crate::schema::StoredPhase::Terminal),
            ))
            .execute()?;
        if updated == 1 {
            Ok(())
        } else {
            Err(DrizzleError::Other(
                "recovery terminal transition lost its phase guard".into(),
            ))
        }
    }};
}

mod compaction;
mod invocation;
mod namespace;
mod recovery;

pub struct DurableInvocationLedger {
    db: StoreDb,
    schema: CommandStoreSchema,
}

impl DurableInvocationLedger {
    /// Opens the file-backed ledger, verifies WAL/FULL durability, and applies
    /// the build-generated migrations.
    ///
    /// # Errors
    ///
    /// Returns [`LedgerError`] when the path cannot be opened, durability
    /// cannot be established, or migrations fail.
    pub fn open(path: &Path) -> Result<Self, LedgerError> {
        let connection = open_sqlite(path)?;
        configure_durability(&connection)?;
        let schema = CommandStoreSchema::new();
        let (db, _) = Drizzle::new(connection, schema);
        let migrations = drizzle::include_migrations!("./drizzle");
        db.migrate(&migrations, Tracking::SQLITE)?;
        Ok(Self { db, schema })
    }
}

fn row_id(row: &InvocationSnapshot) -> InvocationId {
    InvocationId::new(row.namespace.0, row.sequence.0)
}

fn is_missing(error: &DrizzleError) -> bool {
    match error {
        DrizzleError::NotFound | DrizzleError::Rusqlite(rusqlite::Error::QueryReturnedNoRows) => {
            true
        }
        DrizzleError::QueryFailed { source, .. } => is_missing(source),
        _ => false,
    }
}

fn status_from_row(row: InvocationSnapshot) -> InvocationStatus {
    InvocationStatus {
        invocation_id: row_id(&row),
        digest: row.digest.0,
        phase: row.phase.into(),
        logical_revision: row.logical_revision.map(|revision| revision.0),
        terminal_kind: row.terminal_kind.map(Into::into),
        outcome: row.outcome,
        committed_event: row.committed_event,
    }
}

impl From<RecoveryPolicy> for StoredRecoveryPolicy {
    fn from(value: RecoveryPolicy) -> Self {
        match value {
            RecoveryPolicy::ObserveAndConverge => Self::ObserveAndConverge,
            RecoveryPolicy::NeverReplay => Self::NeverReplay,
        }
    }
}

impl From<StoredRecoveryPolicy> for RecoveryPolicy {
    fn from(value: StoredRecoveryPolicy) -> Self {
        match value {
            StoredRecoveryPolicy::ObserveAndConverge => Self::ObserveAndConverge,
            StoredRecoveryPolicy::NeverReplay => Self::NeverReplay,
        }
    }
}

impl From<StoredPhase> for DurablePhase {
    fn from(value: StoredPhase) -> Self {
        match value {
            StoredPhase::Reserved => Self::Reserved,
            StoredPhase::LogicalCommitted => Self::LogicalCommitted,
            StoredPhase::EffectDispatched => Self::EffectDispatched,
            StoredPhase::Terminal => Self::Terminal,
        }
    }
}

impl From<TerminalKind> for StoredTerminalKind {
    fn from(value: TerminalKind) -> Self {
        match value {
            TerminalKind::Succeeded => Self::Succeeded,
            TerminalKind::Failed => Self::Failed,
            TerminalKind::Degraded => Self::Degraded,
            TerminalKind::Indeterminate => Self::Indeterminate,
            TerminalKind::CancelledBeforeCommit => Self::CancelledBeforeCommit,
            TerminalKind::RestartedBeforeCommit => Self::RestartedBeforeCommit,
        }
    }
}

impl From<StoredTerminalKind> for TerminalKind {
    fn from(value: StoredTerminalKind) -> Self {
        match value {
            StoredTerminalKind::Succeeded => Self::Succeeded,
            StoredTerminalKind::Failed => Self::Failed,
            StoredTerminalKind::Degraded => Self::Degraded,
            StoredTerminalKind::Indeterminate => Self::Indeterminate,
            StoredTerminalKind::CancelledBeforeCommit => Self::CancelledBeforeCommit,
            StoredTerminalKind::RestartedBeforeCommit => Self::RestartedBeforeCommit,
        }
    }
}

#[derive(Debug, Error)]
pub enum LedgerError {
    #[error("SQLite driver error: {0}")]
    SQLite(#[from] rusqlite::Error),
    #[error("typed Drizzle operation failed: {0}")]
    Drizzle(#[from] DrizzleError),
    #[error("terminal retention does not fit the ledger's millisecond clock")]
    RetentionOverflow,
}
