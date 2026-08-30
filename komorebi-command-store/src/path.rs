use std::path::Path;

use rusqlite::Connection;

#[cfg(windows)]
pub(crate) fn open_sqlite(path: &Path) -> rusqlite::Result<Connection> {
    use std::ffi::c_int;
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;

    unsafe extern "C" {
        fn sqlite3_open16(
            filename: *const c_void,
            database: *mut *mut rusqlite::ffi::sqlite3,
        ) -> c_int;
    }

    let mut wide_path: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide_path.push(0);

    let mut handle = ptr::null_mut();
    // SAFETY: `wide_path` is NUL-terminated and remains alive for the call;
    // `handle` is a valid out-pointer. No other owner exists yet.
    let result = unsafe { sqlite3_open16(wide_path.as_ptr().cast(), &raw mut handle) };
    if result != rusqlite::ffi::SQLITE_OK {
        let error = rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(result), None);
        if !handle.is_null() {
            // SAFETY: this is the only owner and conversion to `Connection`
            // did not occur, so closing exactly once is required.
            let _ = unsafe { rusqlite::ffi::sqlite3_close(handle) };
        }
        return Err(error);
    }

    // SAFETY: `sqlite3_open16` returned success and this function exclusively
    // owns the handle. `Connection` takes that ownership and closes on drop.
    unsafe { Connection::from_handle_owned(handle) }
}

#[cfg(not(windows))]
pub(crate) fn open_sqlite(path: &Path) -> rusqlite::Result<Connection> {
    Connection::open(path)
}

pub(crate) fn configure_durability(connection: &Connection) -> rusqlite::Result<()> {
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

    Ok(())
}
