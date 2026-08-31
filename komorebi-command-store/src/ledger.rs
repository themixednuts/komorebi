use std::path::Path;

use drizzle::error::DrizzleError;
use drizzle::migrations::Tracking;
use drizzle::sqlite::rusqlite::Drizzle;
use komorebi_protocol::CommandCodecError;
use komorebi_protocol::InvocationId;
use komorebi_protocol::InvocationProgress;
use komorebi_protocol::InvocationStatus;
use komorebi_protocol::InvocationTerminal;
use komorebi_protocol::SettledInvocationKind;
use komorebi_sqlite::open_durable;
use thiserror::Error;

use crate::model::DurableInvocationRecord;
use crate::model::DurablePhase;
use crate::model::RecoveryPolicy;
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
                $crate::schema::UpdateInvocations::default()
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

mod cancellation;
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
        let connection = open_durable(path)?;
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

fn status_from_row(row: InvocationSnapshot) -> Result<DurableInvocationRecord, DrizzleError> {
    let progress = progress_from_row(row.phase, row.state_stamp, row.terminal_kind)?;
    Ok(DurableInvocationRecord {
        status: InvocationStatus::new(row_id(&row), row.digest.0, progress),
        outcome: row.outcome,
        committed_event: row.committed_event,
    })
}

fn progress_from_row(
    phase: StoredPhase,
    state: Option<crate::storage::StoredStateStamp>,
    terminal: Option<StoredTerminalKind>,
) -> Result<InvocationProgress, DrizzleError> {
    match (phase, state, terminal) {
        (StoredPhase::Reserved, None, None) => Ok(InvocationProgress::Reserved),
        (StoredPhase::LogicalCommitted, Some(state), None) => {
            Ok(InvocationProgress::LogicalCommitted(state.0))
        }
        (StoredPhase::EffectDispatched, Some(state), None) => {
            Ok(InvocationProgress::EffectDispatched(state.0))
        }
        (StoredPhase::Terminal, state, Some(terminal)) => {
            terminal_from_row(state, terminal).map(InvocationProgress::Terminal)
        }
        _ => Err(DrizzleError::ConversionError(
            "invocation row has an invalid phase, state, and terminal combination".into(),
        )),
    }
}

fn terminal_from_row(
    state: Option<crate::storage::StoredStateStamp>,
    terminal: StoredTerminalKind,
) -> Result<InvocationTerminal, DrizzleError> {
    let settled = match terminal {
        StoredTerminalKind::Succeeded => Some(SettledInvocationKind::Succeeded),
        StoredTerminalKind::Failed => Some(SettledInvocationKind::Failed),
        StoredTerminalKind::Degraded => Some(SettledInvocationKind::Degraded),
        StoredTerminalKind::Indeterminate => Some(SettledInvocationKind::Indeterminate),
        StoredTerminalKind::CancelledBeforeCommit if state.is_none() => {
            return Ok(InvocationTerminal::CancelledBeforeCommit);
        }
        StoredTerminalKind::RestartedBeforeCommit if state.is_none() => {
            return Ok(InvocationTerminal::RestartedBeforeCommit);
        }
        StoredTerminalKind::CancelledBeforeCommit | StoredTerminalKind::RestartedBeforeCommit => {
            None
        }
    };
    match (state, settled) {
        (Some(state), Some(kind)) => Ok(InvocationTerminal::Settled {
            state: state.0,
            kind,
        }),
        _ => Err(DrizzleError::ConversionError(
            "terminal invocation row has an invalid state and kind combination".into(),
        )),
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

impl From<SettledInvocationKind> for StoredTerminalKind {
    fn from(value: SettledInvocationKind) -> Self {
        match value {
            SettledInvocationKind::Succeeded => Self::Succeeded,
            SettledInvocationKind::Failed => Self::Failed,
            SettledInvocationKind::Degraded => Self::Degraded,
            SettledInvocationKind::Indeterminate => Self::Indeterminate,
        }
    }
}

#[derive(Debug, Error)]
pub enum LedgerError {
    #[error("SQLite driver error: {0}")]
    SQLite(#[from] rusqlite::Error),
    #[error("typed Drizzle operation failed: {0}")]
    Drizzle(#[from] DrizzleError),
    #[error("canonical invocation failed: {0}")]
    InvocationCodec(#[from] CommandCodecError),
    #[error("logical commit state is not the invocation's immediate successor")]
    CommitStateMismatch,
    #[error("durable invocation document failed: {0}")]
    Document(#[from] crate::DocumentError),
    #[error("terminal retention does not fit the ledger's millisecond clock")]
    RetentionOverflow,
}
