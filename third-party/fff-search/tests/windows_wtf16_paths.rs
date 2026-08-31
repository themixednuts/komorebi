#![cfg(windows)]

use std::ffi::OsString;
use std::os::windows::ffi::OsStrExt as _;
use std::os::windows::ffi::OsStringExt as _;

use fff_search::FilePicker;
use fff_search::FilePickerOptions;

#[test]
fn indexed_file_operand_preserves_unpaired_utf16_units() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let root = directory
        .path()
        .join(OsString::from_wide(&[u16::from(b'r'), 0xd801]));
    std::fs::create_dir(&root)?;
    let units = [
        u16::from(b'f'),
        0xd800,
        u16::from(b'.'),
        u16::from(b't'),
        u16::from(b'x'),
        u16::from(b't'),
    ];
    let path = root.join(OsString::from_wide(&units));
    std::fs::write(&path, b"lossless path")?;

    let mut picker = FilePicker::new(FilePickerOptions {
        base_path: root,
        watch: false,
        ..FilePickerOptions::default()
    })?;
    picker.collect_files()?;
    let indexed = picker
        .get_files()
        .first()
        .ok_or("the created file should be indexed")?;
    let resolved = indexed.absolute_path(&picker, picker.base_path());
    assert_eq!(std::fs::read(&resolved)?, b"lossless path");

    let resolved_units = resolved.as_os_str().encode_wide().collect::<Vec<_>>();
    let expected_units = path.as_os_str().encode_wide().collect::<Vec<_>>();
    let resolved_units = resolved_units
        .strip_prefix(&[
            u16::from(b'\\'),
            u16::from(b'\\'),
            u16::from(b'?'),
            u16::from(b'\\'),
        ])
        .unwrap_or(&resolved_units);
    assert_eq!(resolved_units, expected_units);
    Ok(())
}
