use komorebi_settings::SettingsStore;

#[test]
fn file_search_root_is_absent_by_default_and_round_trips_losslessly()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let database = directory.path().join("settings.sqlite");
    let root = file_search_root();

    let mut settings = SettingsStore::open(&database)?;
    assert_eq!(settings.file_search_root()?, None);
    settings.set_file_search_root(&root)?;
    drop(settings);

    let settings = SettingsStore::open(&database)?;
    assert_eq!(settings.file_search_root()?, Some(root));
    Ok(())
}

#[cfg(windows)]
fn file_search_root() -> std::path::PathBuf {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt as _;

    std::path::PathBuf::from(OsString::from_wide(&[
        u16::from(b'E'),
        u16::from(b':'),
        u16::from(b'\\'),
        u16::from(b'f'),
        u16::from(b'i'),
        u16::from(b'l'),
        u16::from(b'e'),
        u16::from(b's'),
        u16::from(b'-'),
        0xD800,
    ]))
}

#[cfg(not(windows))]
fn file_search_root() -> std::path::PathBuf {
    std::path::PathBuf::from("/files")
}
