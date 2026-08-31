#![cfg(windows)]

use std::error::Error;

use komorebi_shell::AppBarCallbackMessage;
use komorebi_shell::WindowsAppBarApi;
use windows::Win32::UI::WindowsAndMessaging::WM_APP;

#[test]
fn current_shell_generation_has_stable_nonzero_native_identity() -> Result<(), Box<dyn Error>> {
    let first = WindowsAppBarApi::shell_generation()?;
    let second = WindowsAppBarApi::shell_generation()?;

    assert_eq!(first, second);
    assert_ne!(first.process_id().get(), 0);
    assert_ne!(first.created_100ns().get(), 0);
    Ok(())
}

#[test]
fn appbar_callback_message_is_confined_to_application_message_space() {
    assert!(AppBarCallbackMessage::new(WM_APP).is_some());
    assert!(AppBarCallbackMessage::new(0xbfff).is_some());
    assert!(AppBarCallbackMessage::new(WM_APP - 1).is_none());
    assert!(AppBarCallbackMessage::new(0xc000).is_none());
}
