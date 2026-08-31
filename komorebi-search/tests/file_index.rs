use komorebi_search::FileIndex;
use komorebi_search::FileSearchLimit;

#[test]
fn file_search_returns_only_index_owned_resolvable_identities()
-> Result<(), Box<dyn std::error::Error>> {
    let first_root = tempfile::tempdir()?;
    let first_path = first_root.path().join("settings.json");
    std::fs::write(&first_path, b"first")?;
    let second_root = tempfile::tempdir()?;
    std::fs::write(second_root.path().join("settings.json"), b"second")?;

    let first = FileIndex::build(first_root.path().to_path_buf())?;
    let second = FileIndex::build(second_root.path().to_path_buf())?;
    let matches = first.search(
        "settngs",
        FileSearchLimit::new(10).ok_or("ten is a valid result limit")?,
    );
    let selected = matches.first().ok_or("typo should match settings.json")?;

    assert_eq!(selected.display_path(), "settings.json");
    assert_eq!(first.resolve(selected.id()), Some(first_path.as_path()));
    assert_eq!(second.resolve(selected.id()), None);
    Ok(())
}

#[cfg(windows)]
#[test]
fn file_search_identity_preserves_unpaired_utf16_root_and_filename()
-> Result<(), Box<dyn std::error::Error>> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStrExt as _;
    use std::os::windows::ffi::OsStringExt as _;

    let directory = tempfile::tempdir()?;
    let root = directory
        .path()
        .join(OsString::from_wide(&[u16::from(b'r'), 0xd801]));
    std::fs::create_dir(&root)?;
    let path = root.join(OsString::from_wide(&[
        u16::from(b'n'),
        0xd800,
        u16::from(b'.'),
        u16::from(b't'),
        u16::from(b'x'),
        u16::from(b't'),
    ]));
    std::fs::write(&path, b"lossless")?;

    let index = FileIndex::build(root)?;
    let matches = index.search(
        "txt",
        FileSearchLimit::new(10).ok_or("ten is a valid result limit")?,
    );
    let selected = matches.first().ok_or("file should be searchable")?;
    let resolved = index.resolve(selected.id()).ok_or("id should resolve")?;

    assert_eq!(std::fs::read(resolved)?, b"lossless");
    let resolved_units = resolved.as_os_str().encode_wide().collect::<Vec<_>>();
    assert!(resolved_units.contains(&0xd801));
    assert!(resolved_units.contains(&0xd800));
    assert!(!resolved_units.contains(&0xfffd));
    Ok(())
}
