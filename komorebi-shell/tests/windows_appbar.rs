#![cfg(windows)]

use std::error::Error;

use komorebi_shell::WindowsAppBarApi;
use komorebi_shell::WindowsAppBarMessages;
use komorebi_shell::WindowsAppBarSignal;
use windows::Win32::Foundation::WPARAM;
use windows::Win32::UI::Shell::ABN_POSCHANGED;
use windows::Win32::UI::WindowsAndMessaging::WM_DISPLAYCHANGE;
use windows::Win32::UI::WindowsAndMessaging::WM_DPICHANGED;
use windows::Win32::UI::WindowsAndMessaging::WM_NCDESTROY;

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
fn documented_window_messages_map_to_typed_appbar_signals() -> Result<(), Box<dyn Error>> {
    let messages = WindowsAppBarMessages::register()?;

    assert_eq!(
        messages.classify(messages.callback().id(), WPARAM(ABN_POSCHANGED as usize)),
        WindowsAppBarSignal::PositionInvalidated
    );
    assert_eq!(
        messages.classify(WM_DISPLAYCHANGE, WPARAM(0)),
        WindowsAppBarSignal::PositionInvalidated
    );
    assert_eq!(
        messages.classify(WM_DPICHANGED, WPARAM(0)),
        WindowsAppBarSignal::PositionInvalidated
    );
    assert_eq!(
        messages.classify(messages.position().id(), WPARAM(0)),
        WindowsAppBarSignal::PositionRequested
    );
    assert_eq!(
        messages.classify(messages.taskbar_created().id(), WPARAM(0)),
        WindowsAppBarSignal::ShellRecreated
    );
    assert_eq!(
        messages.classify(WM_NCDESTROY, WPARAM(0)),
        WindowsAppBarSignal::Destroying
    );
    assert_eq!(
        messages.classify(messages.callback().id(), WPARAM(0)),
        WindowsAppBarSignal::Forward
    );
    Ok(())
}

#[test]
fn appbar_messages_are_registered_by_name_without_numeric_collisions() -> Result<(), Box<dyn Error>>
{
    let first = WindowsAppBarMessages::register()?;
    let second = WindowsAppBarMessages::register()?;

    assert_eq!(first, second);
    assert_ne!(first.callback().id(), first.position().id());
    assert_ne!(first.callback().id(), first.taskbar_created().id());
    assert_ne!(first.position().id(), first.taskbar_created().id());
    assert!((0xc000..=0xffff).contains(&first.callback().id()));
    Ok(())
}
