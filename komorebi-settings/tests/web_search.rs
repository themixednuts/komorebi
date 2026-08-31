use komorebi_settings::SettingsStore;
use komorebi_shell::WebSearchEndpoint;

#[test]
fn web_search_endpoint_is_absent_by_default_and_durable_after_upsert()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let database = directory.path().join("settings.sqlite");
    let first = WebSearchEndpoint::new("https://first.example/search", "q")?;
    let second = WebSearchEndpoint::new("https://second.example/find?source=komorebi", "query")?;

    let mut settings = SettingsStore::open(&database)?;
    assert_eq!(settings.web_search()?, None);
    settings.set_web_search(&first)?;
    settings.set_web_search(&second)?;
    drop(settings);

    let settings = SettingsStore::open(&database)?;
    assert_eq!(settings.web_search()?, Some(second));
    Ok(())
}

#[cfg(windows)]
#[test]
fn settings_database_rejects_unpaired_utf16_without_changing_identity()
-> Result<(), Box<dyn std::error::Error>> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    let directory = tempfile::tempdir()?;
    let filename_units = [
        u16::from(b's'),
        u16::from(b'e'),
        u16::from(b't'),
        u16::from(b't'),
        u16::from(b'i'),
        u16::from(b'n'),
        u16::from(b'g'),
        u16::from(b's'),
        u16::from(b'-'),
        0xD800,
        u16::from(b'.'),
        u16::from(b'd'),
        u16::from(b'b'),
    ];
    let database = directory.path().join(OsString::from_wide(&filename_units));
    let error = SettingsStore::open(&database)
        .err()
        .ok_or("SQLite must reject a path it cannot represent instead of changing its identity")?;
    assert!(matches!(
        error,
        komorebi_settings::SettingsError::SQLite(rusqlite::Error::InvalidPath(path))
            if path == database
    ));
    assert_eq!(std::fs::read_dir(directory.path())?.count(), 0);
    Ok(())
}
