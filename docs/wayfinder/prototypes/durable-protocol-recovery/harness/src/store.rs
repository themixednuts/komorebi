use std::path::Path;
use std::time::Duration;

use drizzle::core::expr::{eq, lt};
use drizzle::error::DrizzleError;
use drizzle::migrations::Tracking;
use drizzle::sqlite::connection::SQLiteTransactionType;
use drizzle::sqlite::pragma::{JournalMode, Pragma, Synchronous};
use drizzle::sqlite::prelude::SQLiteFromRow;
use drizzle::sqlite::rusqlite::Drizzle;
use thiserror::Error;

use crate::domain::{
    DurablePhase, EffectKind, InvalidDigest, InvalidDurablePhase, Invocation, InvocationDigest,
    InvocationId, PrincipalId, RecoveryStatus,
};
use crate::schema::{
    InsertCommittedEventRow, InsertInvocationLedgerRow, InsertPrincipalFloorRow,
    InvocationDocument, ProtocolSchema, SelectInvocationLedgerRow, SelectPrincipalFloorRow,
    UpdateInvocationLedgerRow, UpdatePrincipalFloorRow,
};

pub struct DurableStore {
    database: Drizzle<ProtocolSchema>,
    schema: ProtocolSchema,
}

#[derive(SQLiteFromRow)]
struct JournalModeResult {
    journal_mode: String,
}

impl DurableStore {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let parent = path
            .parent()
            .ok_or_else(|| StoreError::NoParent(path.to_owned()))?;
        std::fs::create_dir_all(parent).map_err(StoreError::CreateParent)?;
        let connection = rusqlite::Connection::open(path).map_err(StoreError::Open)?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(StoreError::Configure)?;
        let (database, schema) = Drizzle::new(connection, ProtocolSchema::new());
        let mode: JournalModeResult = database
            .get(Pragma::JournalMode(JournalMode::Wal))
            .map_err(StoreError::Drizzle)?;
        if !mode.journal_mode.eq_ignore_ascii_case("wal") {
            return Err(StoreError::UnexpectedJournalMode(mode.journal_mode));
        }
        database
            .execute(Pragma::Synchronous(Synchronous::Full))
            .map_err(StoreError::Configure)?;
        database
            .migrate(&drizzle::include_migrations!("./drizzle"), Tracking::SQLITE)
            .map_err(StoreError::Drizzle)?;
        Ok(Self { database, schema })
    }

    pub fn reserve(&self, invocation: &Invocation) -> Result<(), StoreError> {
        self.database
            .insert(self.schema.invocations)
            .value(InsertInvocationLedgerRow::new(
                invocation.identity(),
                invocation.principal.as_str().to_owned(),
                sqlite_integer(invocation.id)?,
                invocation.digest.bytes().to_vec(),
                DurablePhase::Reserved.as_str().to_owned(),
                0,
                invocation.effect.as_str().to_owned(),
                InvocationDocument::new(invocation.parameters.clone()).map_err(StoreError::Json)?,
            ))
            .execute()
            .map_err(StoreError::Drizzle)?;
        Ok(())
    }

    pub fn commit_logical(&self, invocation: &Invocation, revision: u64) -> Result<(), StoreError> {
        self.transition(invocation, DurablePhase::LogicalCommitted, revision)
    }

    pub fn mark_dispatched(
        &self,
        invocation: &Invocation,
        revision: u64,
    ) -> Result<(), StoreError> {
        self.transition(invocation, DurablePhase::EffectDispatched, revision)
    }

    pub fn record_terminal(
        &mut self,
        invocation: &Invocation,
        revision: u64,
        event_position: u64,
    ) -> Result<(), StoreError> {
        let schema = self.schema;
        let identity = invocation.identity();
        let revision = sqlite_u64(revision, "manager revision")?;
        let position = sqlite_u64(event_position, "event position")?;
        self.database
            .transaction(SQLiteTransactionType::Immediate, |tx| {
                tx.update(schema.invocations)
                    .set(
                        UpdateInvocationLedgerRow::default()
                            .with_phase(DurablePhase::Terminal.as_str().to_owned())
                            .with_manager_revision(revision)
                            .with_outcome("applied".to_owned()),
                    )
                    .r#where(eq(schema.invocations.identity, identity.clone()))
                    .execute()?;
                tx.insert(schema.events)
                    .value(
                        InsertCommittedEventRow::new(
                            [0xA5_u8; 16].to_vec(),
                            revision,
                            identity,
                            "action-applied".to_owned(),
                        )
                        .with_position(position),
                    )
                    .execute()?;
                Ok(())
            })
            .map_err(StoreError::Drizzle)
    }

    pub fn recover(&self, invocation: &Invocation) -> Result<RecoveryStatus, StoreError> {
        let rows: Vec<SelectInvocationLedgerRow> = self
            .database
            .select(())
            .from(self.schema.invocations)
            .r#where(eq(self.schema.invocations.identity, invocation.identity()))
            .all()
            .map_err(StoreError::Drizzle)?;
        let Some(row) = zero_or_one(rows, "invocation ledger row")? else {
            return Ok(
                if invocation.id.value() < self.minimum_accepted(&invocation.principal)? {
                    RecoveryStatus::InvocationExpired
                } else {
                    RecoveryStatus::NotReserved
                },
            );
        };
        if row.principal != invocation.principal.as_str()
            || u64::try_from(row.invocation_id).ok() != Some(invocation.id.value())
            || InvocationDigest::from_slice(&row.digest)? != invocation.digest
            || row.parameters.parameters() != &invocation.parameters
        {
            return Ok(RecoveryStatus::IdempotencyConflict);
        }
        let phase = DurablePhase::parse(&row.phase)?;
        let effect = match row.effect_kind.as_str() {
            "idempotent-setter" => EffectKind::IdempotentSetter,
            "ambiguous-toggle" => EffectKind::AmbiguousToggle,
            other => return Err(StoreError::InvalidEffect(other.to_owned())),
        };
        Ok(match (phase, effect) {
            (DurablePhase::Reserved, _) => RecoveryStatus::RestartedBeforeCommit,
            (DurablePhase::LogicalCommitted, _)
            | (DurablePhase::EffectDispatched, EffectKind::IdempotentSetter) => {
                RecoveryStatus::ReconcilingAfterRestart
            }
            (DurablePhase::EffectDispatched, EffectKind::AmbiguousToggle) => {
                RecoveryStatus::Indeterminate
            }
            (DurablePhase::Terminal, _) => RecoveryStatus::RetainedTerminal,
        })
    }

    pub fn compact(
        &mut self,
        principal: &PrincipalId,
        minimum: InvocationId,
    ) -> Result<(), StoreError> {
        let schema = self.schema;
        let minimum = sqlite_integer(minimum)?;
        self.database
            .transaction(SQLiteTransactionType::Immediate, |tx| {
                let existing: Vec<SelectPrincipalFloorRow> = tx
                    .select(())
                    .from(schema.principal_floors)
                    .r#where(eq(
                        schema.principal_floors.principal,
                        principal.as_str().to_owned(),
                    ))
                    .all()?;
                if existing.is_empty() {
                    tx.insert(schema.principal_floors)
                        .value(InsertPrincipalFloorRow::new(
                            principal.as_str().to_owned(),
                            minimum,
                        ))
                        .execute()?;
                } else {
                    tx.update(schema.principal_floors)
                        .set(UpdatePrincipalFloorRow::default().with_minimum_accepted(minimum))
                        .r#where(eq(
                            schema.principal_floors.principal,
                            principal.as_str().to_owned(),
                        ))
                        .execute()?;
                }
                tx.delete(schema.invocations)
                    .r#where((
                        eq(schema.invocations.principal, principal.as_str().to_owned()),
                        lt(schema.invocations.invocation_id, minimum),
                    ))
                    .execute()?;
                Ok(())
            })
            .map_err(StoreError::Drizzle)
    }

    fn transition(
        &self,
        invocation: &Invocation,
        phase: DurablePhase,
        revision: u64,
    ) -> Result<(), StoreError> {
        self.database
            .update(self.schema.invocations)
            .set(
                UpdateInvocationLedgerRow::default()
                    .with_phase(phase.as_str().to_owned())
                    .with_manager_revision(sqlite_u64(revision, "manager revision")?),
            )
            .r#where(eq(self.schema.invocations.identity, invocation.identity()))
            .execute()
            .map_err(StoreError::Drizzle)?;
        Ok(())
    }

    fn minimum_accepted(&self, principal: &PrincipalId) -> Result<u64, StoreError> {
        let rows: Vec<SelectPrincipalFloorRow> = self
            .database
            .select(())
            .from(self.schema.principal_floors)
            .r#where(eq(
                self.schema.principal_floors.principal,
                principal.as_str().to_owned(),
            ))
            .all()
            .map_err(StoreError::Drizzle)?;
        let Some(row) = zero_or_one(rows, "principal floor")? else {
            return Ok(0);
        };
        u64::try_from(row.minimum_accepted).map_err(|_| StoreError::Range("principal floor"))
    }
}

fn sqlite_integer(id: InvocationId) -> Result<i64, StoreError> {
    sqlite_u64(id.value(), "invocation id")
}

fn sqlite_u64(value: u64, field: &'static str) -> Result<i64, StoreError> {
    i64::try_from(value).map_err(|_| StoreError::Range(field))
}

fn zero_or_one<T>(rows: Vec<T>, entity: &'static str) -> Result<Option<T>, StoreError> {
    let mut rows = rows.into_iter();
    let first = rows.next();
    if rows.next().is_some() {
        return Err(StoreError::Duplicate(entity));
    }
    Ok(first)
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("durable store path has no parent: {0:?}")]
    NoParent(std::path::PathBuf),
    #[error("create durable store parent")]
    CreateParent(#[source] std::io::Error),
    #[error("open SQLite durable store")]
    Open(#[source] rusqlite::Error),
    #[error("configure SQLite durable store")]
    Configure(#[source] rusqlite::Error),
    #[error("SQLite refused WAL journal mode and selected {0}")]
    UnexpectedJournalMode(String),
    #[error("Drizzle durable-store operation")]
    Drizzle(#[source] DrizzleError),
    #[error("duplicate {0}")]
    Duplicate(&'static str),
    #[error("durable-store integer is outside domain range: {0}")]
    Range(&'static str),
    #[error(transparent)]
    InvalidDigest(#[from] InvalidDigest),
    #[error(transparent)]
    InvalidPhase(#[from] InvalidDurablePhase),
    #[error("unknown durable effect kind {0:?}")]
    InvalidEffect(String),
    #[error("decode typed BLOB action parameters")]
    Json(#[source] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::DurableStore;
    use crate::domain::{
        EffectKind, Invocation, InvocationDigest, InvocationId, PrincipalId, RecoveryStatus,
    };
    use crate::schema::InvocationParameters;

    #[test]
    fn generated_select_row_decodes_custom_blob_column() -> Result<(), Box<dyn std::error::Error>> {
        let temporary = tempfile::tempdir()?;
        let parameters = InvocationParameters {
            schema: 1,
            action: "window.focus".to_owned(),
            arguments: vec!["次".to_owned(), r"C:\workspace\δ".to_owned()],
        };
        let invocation = Invocation {
            principal: PrincipalId::parse("test-principal")?,
            id: InvocationId::new(7),
            digest: InvocationDigest::canonical(&parameters)?,
            parameters,
            effect: EffectKind::IdempotentSetter,
        };
        let store = DurableStore::open(&temporary.path().join("state.sqlite3"))?;
        store.reserve(&invocation)?;

        assert_eq!(
            store.recover(&invocation)?,
            RecoveryStatus::RestartedBeforeCommit
        );
        Ok(())
    }
}
