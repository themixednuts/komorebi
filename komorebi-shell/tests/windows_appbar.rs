#![cfg(windows)]

use std::error::Error;

use komorebi_shell::WindowsAppBarApi;
use komorebi_shell::WindowsAppBarMessages;

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
