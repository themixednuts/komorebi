mod schema;

use anyhow::{Context as _, Result, ensure};
use drizzle::{
    core::expr::eq,
    migrations::Tracking,
    sqlite::{
        pragma::{JournalMode, Pragma},
        prelude::SQLiteFromRow,
        rusqlite::Drizzle,
    },
};
use schema::{CompatibilitySchema, InsertToolchainFactRow, SelectToolchainFactRow};
use serde::Serialize;
use windows_sys::Win32::System::Threading::GetCurrentProcessId;

#[derive(Debug, Serialize)]
pub struct StateCompatibility {
    pub drizzle_query_label: String,
    pub drizzle_blob_bytes: usize,
    pub sqlite_journal_mode: String,
    pub windows_process_id: u32,
}

#[derive(SQLiteFromRow)]
struct JournalModeResult {
    journal_mode: String,
}

pub fn run() -> Result<StateCompatibility> {
    let connection = rusqlite::Connection::open_in_memory().context("open in-memory SQLite")?;
    let (database, schema) = Drizzle::new(connection, CompatibilitySchema::new());
    let journal: JournalModeResult = database
        .get(Pragma::JournalMode(JournalMode::Memory))
        .context("select journal mode through FromSQLiteRow")?;
    database
        .migrate(&drizzle::include_migrations!("./drizzle"), Tracking::SQLITE)
        .context("apply build.rs-generated migration")?;
    let native_payload = vec![0x5c, 0x00, 0x00, 0xd8, 0x2d, 0x00];
    database
        .insert(schema.facts)
        .value(InsertToolchainFactRow::new(
            "nightly-simd-compatibility".to_owned(),
            native_payload.clone(),
        ))
        .execute()
        .context("insert through Drizzle query API")?;
    let rows: Vec<SelectToolchainFactRow> = database
        .select(())
        .from(schema.facts)
        .r#where(eq(
            schema.facts.label,
            "nightly-simd-compatibility".to_owned(),
        ))
        .all()
        .context("select through Drizzle query API")?;
    let [row] = rows.as_slice() else {
        anyhow::bail!("expected one typed Drizzle row, found {}", rows.len());
    };
    ensure!(
        row.native_payload == native_payload,
        "SQLite blob changed bytes"
    );

    // SAFETY: GetCurrentProcessId has no preconditions and returns a value owned by Windows.
    let process_id = unsafe { GetCurrentProcessId() };
    Ok(StateCompatibility {
        drizzle_query_label: row.label.clone(),
        drizzle_blob_bytes: row.native_payload.len(),
        sqlite_journal_mode: journal.journal_mode,
        windows_process_id: process_id,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn drizzle_queries_migrations_and_windows_adapter_compile_and_run()
    -> Result<(), Box<dyn std::error::Error>> {
        let report = super::run()?;
        assert_eq!(report.drizzle_query_label, "nightly-simd-compatibility");
        assert_eq!(report.drizzle_blob_bytes, 6);
        assert_ne!(report.windows_process_id, 0);
        Ok(())
    }
}
