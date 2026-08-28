use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::Path;
use std::time::Duration;

use drizzle::core::expr::eq;
use drizzle::error::DrizzleError;
use drizzle::migrations::Tracking;
use drizzle::sqlite::connection::SQLiteTransactionType;
use drizzle::sqlite::pragma::{JournalMode, Pragma, Synchronous};
use drizzle::sqlite::prelude::{SQLiteFromRow, asc};
use drizzle::sqlite::rusqlite::Drizzle;
use thiserror::Error;

use crate::installation::{CandidateSeal, MigratedConfiguration, WindowPlacement, WindowSnapshot};
use crate::schema::{
    InsertCandidateSealRow, InsertConfigurationBindingRow, InsertConfigurationWorkspaceRow,
    InsertInternalConfigurationRow, InsertNativePathFactRow, InsertWindowRecoveryPlacementRow,
    InsertWindowRecoverySnapshotRow, PromotionSchema, SelectCandidateSealRow,
    SelectConfigurationBindingRow, SelectConfigurationWorkspaceRow, SelectInternalConfigurationRow,
    SelectNativePathFactRow, SelectWindowRecoveryPlacementRow, SelectWindowRecoverySnapshotRow,
    UpdateWindowRecoverySnapshotRow,
};

pub struct Store {
    pub(crate) database: Drizzle<PromotionSchema>,
    pub(crate) schema: PromotionSchema,
}

#[derive(SQLiteFromRow)]
struct JournalModeResult {
    journal_mode: String,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let parent = path
            .parent()
            .ok_or_else(|| StoreError::NoParent(path.to_owned()))?;
        std::fs::create_dir_all(parent).map_err(StoreError::CreateParent)?;
        let connection = rusqlite::Connection::open(path).map_err(StoreError::Open)?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(StoreError::Configure)?;
        let (database, schema) = Drizzle::new(connection, PromotionSchema::new());
        let journal_mode: JournalModeResult = database
            .get(Pragma::JournalMode(JournalMode::Wal))
            .map_err(StoreError::Drizzle)?;
        if !journal_mode.journal_mode.eq_ignore_ascii_case("wal") {
            return Err(StoreError::UnexpectedJournalMode(journal_mode.journal_mode));
        }
        database
            .execute(Pragma::Synchronous(Synchronous::Full))
            .map_err(StoreError::Configure)?;
        let migrations = drizzle::include_migrations!("./drizzle");
        database
            .migrate(&migrations, Tracking::SQLITE)
            .map_err(StoreError::Drizzle)?;
        Ok(Self { database, schema })
    }

    pub fn put_native_path(&self, transaction: &str, path: &Path) -> Result<(), StoreError> {
        let mut code_units = Vec::new();
        for unit in path.as_os_str().encode_wide() {
            code_units.extend_from_slice(&unit.to_le_bytes());
        }
        self.database
            .insert(self.schema.native_paths)
            .value(InsertNativePathFactRow::new(
                transaction.to_owned(),
                1,
                code_units,
            ))
            .execute()
            .map_err(StoreError::Drizzle)?;
        Ok(())
    }

    pub fn native_path(&self, transaction: &str) -> Result<OsString, StoreError> {
        let row: SelectNativePathFactRow = exactly_one(
            self.database
                .select(())
                .from(self.schema.native_paths)
                .r#where(eq(
                    self.schema.native_paths.transaction,
                    transaction.to_owned(),
                ))
                .all()
                .map_err(StoreError::Drizzle)?,
            "native path fact",
        )?;
        if row.encoding_version != 1 || !row.code_units.len().is_multiple_of(2) {
            return Err(StoreError::NativePathEncoding);
        }
        let code_units = row
            .code_units
            .chunks_exact(2)
            .map(|bytes| {
                let pair: [u8; 2] = bytes
                    .try_into()
                    .map_err(|_| StoreError::NativePathEncoding)?;
                Ok(u16::from_le_bytes(pair))
            })
            .collect::<Result<Vec<_>, StoreError>>()?;
        Ok(OsString::from_wide(&code_units))
    }

    pub fn put_configuration(
        &mut self,
        transaction: &str,
        configuration: &MigratedConfiguration,
    ) -> Result<(), StoreError> {
        let schema = self.schema;
        self.database
            .transaction(SQLiteTransactionType::Immediate, |tx| {
                tx.insert(schema.configurations)
                    .value(InsertInternalConfigurationRow::new(
                        transaction.to_owned(),
                        i64::from(configuration.schema),
                        configuration.source_digest.clone(),
                    ))
                    .execute()?;
                for (position, workspace) in configuration.workspaces.iter().enumerate() {
                    tx.insert(schema.configuration_workspaces)
                        .value(InsertConfigurationWorkspaceRow::new(
                            transaction.to_owned(),
                            i64::try_from(position).map_err(|_| {
                                DrizzleError::Other(
                                    "workspace position exceeds SQLite INTEGER".into(),
                                )
                            })?,
                            workspace.clone(),
                        ))
                        .execute()?;
                }
                for (position, workspace_position) in configuration.bindings.iter().enumerate() {
                    tx.insert(schema.configuration_bindings)
                        .value(InsertConfigurationBindingRow::new(
                            transaction.to_owned(),
                            i64::try_from(position).map_err(|_| {
                                DrizzleError::Other(
                                    "binding position exceeds SQLite INTEGER".into(),
                                )
                            })?,
                            i64::from(*workspace_position),
                        ))
                        .execute()?;
                }
                Ok(())
            })
            .map_err(StoreError::Drizzle)
    }

    pub fn configuration(&self, transaction: &str) -> Result<MigratedConfiguration, StoreError> {
        let schema = self.schema;
        let header: SelectInternalConfigurationRow = exactly_one(
            self.database
                .select(())
                .from(schema.configurations)
                .r#where(eq(
                    schema.configurations.transaction,
                    transaction.to_owned(),
                ))
                .all()
                .map_err(StoreError::Drizzle)?,
            "internal configuration",
        )?;
        let workspaces: Vec<SelectConfigurationWorkspaceRow> = self
            .database
            .select(())
            .from(schema.configuration_workspaces)
            .r#where(eq(
                schema.configuration_workspaces.transaction,
                transaction.to_owned(),
            ))
            .order_by(asc(schema.configuration_workspaces.position))
            .all()
            .map_err(StoreError::Drizzle)?;
        let bindings: Vec<SelectConfigurationBindingRow> = self
            .database
            .select(())
            .from(schema.configuration_bindings)
            .r#where(eq(
                schema.configuration_bindings.transaction,
                transaction.to_owned(),
            ))
            .order_by(asc(schema.configuration_bindings.position))
            .all()
            .map_err(StoreError::Drizzle)?;
        Ok(MigratedConfiguration {
            schema: u8::try_from(header.schema_version)
                .map_err(|_| StoreError::Range("configuration schema is outside u8"))?,
            workspaces: workspaces.into_iter().map(|row| row.name).collect(),
            bindings: bindings
                .into_iter()
                .map(|row| {
                    u8::try_from(row.workspace_position)
                        .map_err(|_| StoreError::Range("binding workspace is outside u8"))
                })
                .collect::<Result<Vec<_>, _>>()?,
            source_digest: header.source_digest,
        })
    }

    pub fn put_candidate_seal(
        &self,
        transaction: &str,
        seal: &CandidateSeal,
    ) -> Result<(), StoreError> {
        self.database
            .insert(self.schema.candidate_seals)
            .value(InsertCandidateSealRow::new(
                transaction.to_owned(),
                seal.installation.as_str().to_owned(),
                seal.payload_digest.clone(),
                seal.configuration_digest.clone(),
            ))
            .execute()
            .map_err(StoreError::Drizzle)?;
        Ok(())
    }

    pub fn candidate_seal(&self, transaction: &str) -> Result<CandidateSeal, StoreError> {
        let row: SelectCandidateSealRow = exactly_one(
            self.database
                .select(())
                .from(self.schema.candidate_seals)
                .r#where(eq(
                    self.schema.candidate_seals.transaction,
                    transaction.to_owned(),
                ))
                .all()
                .map_err(StoreError::Drizzle)?,
            "candidate seal",
        )?;
        Ok(CandidateSeal {
            installation: row
                .installation
                .parse()
                .map_err(StoreError::InstallationId)?,
            payload_digest: row.payload_digest,
            configuration_digest: row.configuration_digest,
        })
    }

    pub fn put_window_snapshot(
        &mut self,
        transaction: &str,
        snapshot: &WindowSnapshot,
    ) -> Result<(), StoreError> {
        let schema = self.schema;
        self.database
            .transaction(SQLiteTransactionType::Immediate, |tx| {
                tx.insert(schema.window_snapshots)
                    .value(InsertWindowRecoverySnapshotRow::new(
                        transaction.to_owned(),
                        snapshot.focused.clone(),
                        snapshot.appearance_digest.clone(),
                        false,
                    ))
                    .execute()?;
                for placement in &snapshot.windows {
                    let [x, y, width, height] = placement.frame;
                    tx.insert(schema.window_placements)
                        .value(InsertWindowRecoveryPlacementRow::new(
                            transaction.to_owned(),
                            placement.identity.clone(),
                            i64::from(x),
                            i64::from(y),
                            i64::from(width),
                            i64::from(height),
                        ))
                        .execute()?;
                }
                Ok(())
            })
            .map_err(StoreError::Drizzle)
    }

    pub fn window_snapshot(&self, transaction: &str) -> Result<WindowSnapshot, StoreError> {
        let schema = self.schema;
        let header: SelectWindowRecoverySnapshotRow = exactly_one(
            self.database
                .select(())
                .from(schema.window_snapshots)
                .r#where(eq(
                    schema.window_snapshots.transaction,
                    transaction.to_owned(),
                ))
                .all()
                .map_err(StoreError::Drizzle)?,
            "window recovery snapshot",
        )?;
        let placements: Vec<SelectWindowRecoveryPlacementRow> = self
            .database
            .select(())
            .from(schema.window_placements)
            .r#where(eq(
                schema.window_placements.transaction,
                transaction.to_owned(),
            ))
            .order_by(asc(schema.window_placements.id))
            .all()
            .map_err(StoreError::Drizzle)?;
        let windows = placements
            .into_iter()
            .map(|row| {
                Ok(WindowPlacement {
                    identity: row.window_identity,
                    frame: [
                        i32::try_from(row.x).map_err(|_| StoreError::Range("window x"))?,
                        i32::try_from(row.y).map_err(|_| StoreError::Range("window y"))?,
                        i32::try_from(row.width).map_err(|_| StoreError::Range("window width"))?,
                        i32::try_from(row.height)
                            .map_err(|_| StoreError::Range("window height"))?,
                    ],
                })
            })
            .collect::<Result<Vec<_>, StoreError>>()?;
        Ok(WindowSnapshot {
            windows,
            focused: header.focused_window,
            appearance_digest: header.appearance_digest,
        })
    }

    pub fn mark_snapshot_reconciled(&self, transaction: &str) -> Result<(), StoreError> {
        self.database
            .update(self.schema.window_snapshots)
            .set(UpdateWindowRecoverySnapshotRow::default().with_reconciled(true))
            .r#where(eq(
                self.schema.window_snapshots.transaction,
                transaction.to_owned(),
            ))
            .execute()
            .map_err(StoreError::Drizzle)?;
        Ok(())
    }

    pub fn delete_window_snapshot(&mut self, transaction: &str) -> Result<(), StoreError> {
        let schema = self.schema;
        self.database
            .transaction(SQLiteTransactionType::Immediate, |tx| {
                tx.delete(schema.window_placements)
                    .r#where(eq(
                        schema.window_placements.transaction,
                        transaction.to_owned(),
                    ))
                    .execute()?;
                tx.delete(schema.window_snapshots)
                    .r#where(eq(
                        schema.window_snapshots.transaction,
                        transaction.to_owned(),
                    ))
                    .execute()?;
                Ok(())
            })
            .map_err(StoreError::Drizzle)
    }
}

fn exactly_one<T>(rows: Vec<T>, entity: &'static str) -> Result<T, StoreError> {
    let mut rows = rows.into_iter();
    let first = rows.next().ok_or(StoreError::Missing(entity))?;
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
    #[error("missing {0}")]
    Missing(&'static str),
    #[error("duplicate {0}")]
    Duplicate(&'static str),
    #[error("durable-store integer is outside domain range: {0}")]
    Range(&'static str),
    #[error("native path fact has an unsupported or malformed encoding")]
    NativePathEncoding,
    #[error("invalid installation identifier in durable store")]
    InstallationId(#[source] crate::domain::InvalidInstallationId),
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};

    use super::Store;

    #[test]
    fn native_path_round_trip_preserves_unpaired_utf16() -> Result<(), Box<dyn std::error::Error>> {
        let temporary = tempfile::tempdir()?;
        let mut native_units = "fixture-".encode_utf16().collect::<Vec<_>>();
        native_units.push(0xD800);
        native_units.extend("-path".encode_utf16());
        let native_path = temporary.path().join(OsString::from_wide(&native_units));
        let store = Store::open(&temporary.path().join("manager-state.sqlite3"))?;

        store.put_native_path("transaction-1", &native_path)?;
        let loaded = store.native_path("transaction-1")?;

        assert_eq!(
            loaded.encode_wide().collect::<Vec<_>>(),
            native_path.as_os_str().encode_wide().collect::<Vec<_>>()
        );
        Ok(())
    }
}
