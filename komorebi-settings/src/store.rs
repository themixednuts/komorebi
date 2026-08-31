use std::path::Path;
use std::path::PathBuf;

use drizzle::core::expr::eq;
use drizzle::core::expr::excluded;
use drizzle::error::DrizzleError;
use drizzle::migrations::Tracking;
use drizzle::sqlite::rusqlite::Drizzle;
use komorebi_shell::WebSearchEndpoint;
use komorebi_shell::WebSearchEndpointError;
use komorebi_sqlite::open_durable;
use thiserror::Error;

use crate::path::PathEncodingError;
use crate::schema::FileSearchRow;
use crate::schema::InsertFileSearchSettings;
use crate::schema::InsertWebSearchSettings;
use crate::schema::SettingsSchema;
use crate::schema::UpdateFileSearchSettings;
use crate::schema::UpdateWebSearchSettings;
use crate::schema::WebSearchRow;

const SETTINGS_SINGLETON: i64 = 1;

type SettingsDb = Drizzle<SettingsSchema>;

/// File-backed typed settings with build-generated schema migrations.
pub struct SettingsStore {
    db: SettingsDb,
    schema: SettingsSchema,
}

impl SettingsStore {
    /// Opens the durable settings database and applies embedded migrations.
    ///
    /// # Errors
    ///
    /// Returns when the native path cannot be opened, the durability profile
    /// cannot be established, or a generated migration fails.
    pub fn open(path: &Path) -> Result<Self, SettingsError> {
        let connection = open_durable(path)?;
        let schema = SettingsSchema::new();
        let (db, _) = Drizzle::new(connection, schema);
        let migrations = drizzle::include_migrations!("./drizzle");
        db.migrate(&migrations, Tracking::SQLITE)?;
        Ok(Self { db, schema })
    }

    /// Loads and validates the configured web-search authority.
    ///
    /// # Errors
    ///
    /// Returns a typed query failure or rejects a persisted endpoint that no
    /// longer satisfies the shell's authority invariant.
    pub fn web_search(&self) -> Result<Option<WebSearchEndpoint>, SettingsError> {
        let table = self.schema.web_search;
        let row: Result<WebSearchRow, DrizzleError> = self
            .db
            .select((table.base_url, table.query_parameter))
            .from(table)
            .r#where(eq(table.singleton, SETTINGS_SINGLETON))
            .get();
        match row {
            Ok(row) => WebSearchEndpoint::new(&row.base_url, &row.query_parameter)
                .map(Some)
                .map_err(Into::into),
            Err(error) if is_missing(&error) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// Atomically inserts or replaces the configured web-search authority.
    ///
    /// # Errors
    ///
    /// Returns a typed Drizzle failure without changing the prior row.
    pub fn set_web_search(&mut self, endpoint: &WebSearchEndpoint) -> Result<(), SettingsError> {
        let table = self.schema.web_search;
        self.db
            .insert(table)
            .values([InsertWebSearchSettings::new(
                endpoint.base_url().to_owned(),
                endpoint.query_parameter().to_owned(),
            )
            .with_singleton(SETTINGS_SINGLETON)])
            .on_conflict(table.singleton)
            .do_update(
                UpdateWebSearchSettings::default()
                    .with_base_url(excluded(table.base_url))
                    .with_query_parameter(excluded(table.query_parameter)),
            )
            .execute()?;
        Ok(())
    }

    /// Loads the exact root of the optional first-party file index.
    ///
    /// # Errors
    ///
    /// Returns a typed query or stored-path representation failure.
    pub fn file_search_root(&self) -> Result<Option<PathBuf>, SettingsError> {
        let table = self.schema.file_search;
        let row: Result<FileSearchRow, DrizzleError> = self
            .db
            .select(table.root_wtf16)
            .from(table)
            .r#where(eq(table.singleton, SETTINGS_SINGLETON))
            .get();
        match row {
            Ok(row) => crate::path::decode(&row.root_wtf16)
                .map(Some)
                .map_err(Into::into),
            Err(error) if is_missing(&error) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// Atomically inserts or replaces the exact first-party file-index root.
    ///
    /// # Errors
    ///
    /// Returns a typed Drizzle failure without changing the prior row.
    pub fn set_file_search_root(&mut self, root: &Path) -> Result<(), SettingsError> {
        let table = self.schema.file_search;
        self.db
            .insert(table)
            .values([InsertFileSearchSettings::new(crate::path::encode(root))
                .with_singleton(SETTINGS_SINGLETON)])
            .on_conflict(table.singleton)
            .do_update(
                UpdateFileSearchSettings::default().with_root_wtf16(excluded(table.root_wtf16)),
            )
            .execute()?;
        Ok(())
    }
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

/// Durable settings failure translated at the persistence boundary.
#[derive(Debug, Error)]
pub enum SettingsError {
    #[error("SQLite driver error: {0}")]
    SQLite(#[from] rusqlite::Error),
    #[error("typed Drizzle operation failed: {0}")]
    Drizzle(#[from] DrizzleError),
    #[error("persisted web-search configuration is invalid: {0}")]
    WebSearch(#[from] WebSearchEndpointError),
    #[error("persisted WTF-16 path contains an odd byte count: {byte_length}")]
    PathEncoding { byte_length: usize },
}

impl From<PathEncodingError> for SettingsError {
    fn from(error: PathEncodingError) -> Self {
        match error {
            PathEncodingError::OddUtf16ByteLength(byte_length) => {
                Self::PathEncoding { byte_length }
            }
        }
    }
}
