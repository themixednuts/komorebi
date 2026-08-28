use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;

use crate::windows::wide;

#[test]
fn win32_strings_preserve_wtf16_and_reject_interior_nul() {
    let ill_formed = OsString::from_wide(&[u16::from(b'x'), 0xd800, u16::from(b'y')]);
    let encoded = wide(&ill_formed).expect("ill-formed UTF-16 is valid Windows path data");
    assert_eq!(
        encoded.as_slice_with_nul(),
        &[u16::from(b'x'), 0xd800, u16::from(b'y'), 0]
    );

    let truncated_by_raw_win32 = OsString::from_wide(&[u16::from(b'x'), 0, u16::from(b'y')]);
    assert!(wide(&truncated_by_raw_win32).is_err());
}
