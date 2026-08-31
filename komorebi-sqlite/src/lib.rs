//! Shared `SQLite` opening and durability policy.

use std::path::Path;

use rusqlite::Connection;

/// Opens a file-backed `SQLite` database and verifies its durability profile.
///
/// `SQLite` converts even `sqlite3_open16` filenames through UTF-8 internally.
/// Consequently, an unpaired-WTF-16 Windows path is rejected as
/// [`rusqlite::Error::InvalidPath`] instead of being silently mapped to a
/// different filesystem identity.
///
/// # Errors
///
/// Returns when the path is not representable by `SQLite` or `SQLite` cannot enable
/// and confirm WAL, full synchronization, and foreign-key enforcement.
pub fn open_durable(path: &Path) -> rusqlite::Result<Connection> {
    let connection = Connection::open(path)?;
    let journal_mode: String =
        connection.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))?;
    if !journal_mode.eq_ignore_ascii_case("wal") {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "journal_mode={journal_mode}"
        )));
    }

    connection.pragma_update(None, "synchronous", "FULL")?;
    connection.pragma_update(None, "foreign_keys", true)?;

    let synchronous: i64 = connection.pragma_query_value(None, "synchronous", |row| row.get(0))?;
    if synchronous != 2 {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "synchronous={synchronous}"
        )));
    }

    let foreign_keys: i64 =
        connection.pragma_query_value(None, "foreign_keys", |row| row.get(0))?;
    if foreign_keys != 1 {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "foreign_keys={foreign_keys}"
        )));
    }

    Ok(connection)
}
