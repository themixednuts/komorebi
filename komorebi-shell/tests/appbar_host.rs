use std::cell::RefCell;
use std::collections::VecDeque;
use std::error::Error;
use std::fmt;
use std::num::NonZeroU32;
use std::num::NonZeroU64;
use std::rc::Rc;

use komorebi_shell::AppBarGeometry;
use komorebi_shell::AppBarHost;
use komorebi_shell::AppBarHostPlatform;
use komorebi_shell::AppBarVisibility;
use komorebi_shell::ShellGeneration;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Call {
    ShellGeneration,
    Register,
    SchedulePosition,
    ReserveAndPosition,
    Show,
    Remove,
}

#[derive(Debug)]
struct TestError;

impl fmt::Display for TestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("test platform failure")
    }
}

impl Error for TestError {}

struct FakePlatform {
    shells: VecDeque<ShellGeneration>,
    calls: Rc<RefCell<Vec<Call>>>,
    fail_remove_once: bool,
    fail_schedule_once: bool,
}

impl FakePlatform {
    fn new(shells: impl IntoIterator<Item = ShellGeneration>) -> Self {
        Self {
            shells: shells.into_iter().collect(),
            calls: Rc::new(RefCell::new(Vec::new())),
            fail_remove_once: false,
            fail_schedule_once: false,
        }
    }

    fn failing_first_schedule(shells: impl IntoIterator<Item = ShellGeneration>) -> Self {
        Self {
            fail_schedule_once: true,
            ..Self::new(shells)
        }
    }

    fn failing_first_removal(shells: impl IntoIterator<Item = ShellGeneration>) -> Self {
        Self {
            fail_remove_once: true,
            ..Self::new(shells)
        }
    }

    fn call_log(&self) -> Rc<RefCell<Vec<Call>>> {
        Rc::clone(&self.calls)
    }
}

impl AppBarHostPlatform for FakePlatform {
    type Error = TestError;

    fn shell_generation(&mut self) -> Result<ShellGeneration, Self::Error> {
        self.calls.borrow_mut().push(Call::ShellGeneration);
        self.shells.pop_front().ok_or(TestError)
    }

    fn register(&mut self) -> Result<(), Self::Error> {
        self.calls.borrow_mut().push(Call::Register);
        Ok(())
    }

    fn schedule_position(&mut self) -> Result<(), Self::Error> {
        self.calls.borrow_mut().push(Call::SchedulePosition);
        if std::mem::take(&mut self.fail_schedule_once) {
            Err(TestError)
        } else {
            Ok(())
        }
    }

    fn position(&mut self, visibility: AppBarVisibility) -> Result<(), Self::Error> {
        self.calls.borrow_mut().push(Call::ReserveAndPosition);
        if visibility == AppBarVisibility::RevealAfterPosition {
            self.calls.borrow_mut().push(Call::Show);
        }
        Ok(())
    }

    fn remove(&mut self) -> Result<(), Self::Error> {
        self.calls.borrow_mut().push(Call::Remove);
        if std::mem::take(&mut self.fail_remove_once) {
            Err(TestError)
        } else {
            Ok(())
        }
    }

    fn update_geometry(&mut self, _geometry: AppBarGeometry) {}
}

#[test]
fn failed_position_schedule_can_be_retried_by_the_next_native_event() -> Result<(), Box<dyn Error>>
{
    let platform = FakePlatform::failing_first_schedule([shell(10, 100)?]);
    let calls = platform.call_log();
    let mut host = AppBarHost::new(platform);

    assert!(host.start().is_err());
    host.position_invalidated()?;

    assert_eq!(
        calls
            .borrow()
            .iter()
            .filter(|call| **call == Call::SchedulePosition)
            .count(),
        2
    );
    Ok(())
}

#[test]
fn failed_removal_can_be_retried_without_reviving_the_host() -> Result<(), Box<dyn Error>> {
    let platform = FakePlatform::failing_first_removal([shell(10, 100)?]);
    let calls = platform.call_log();
    let mut host = AppBarHost::new(platform);
    host.start()?;

    assert!(host.shutdown().is_err());
    host.shutdown()?;

    assert_eq!(
        calls
            .borrow()
            .iter()
            .filter(|call| **call == Call::Remove)
            .count(),
        2
    );
    assert!(host.position_invalidated().is_err());
    Ok(())
}

fn shell(process_id: u32, created_100ns: u64) -> Result<ShellGeneration, Box<dyn Error>> {
    Ok(ShellGeneration::new(
        NonZeroU32::new(process_id).ok_or(TestError)?,
        NonZeroU64::new(created_100ns).ok_or(TestError)?,
    ))
}

#[test]
fn first_frame_waits_for_native_reservation_and_positioning() -> Result<(), Box<dyn Error>> {
    let platform = FakePlatform::new([shell(10, 100)?]);
    let calls = platform.call_log();
    let mut host = AppBarHost::new(platform);

    host.start()?;
    assert_eq!(
        calls.borrow().as_slice(),
        &[
            Call::ShellGeneration,
            Call::Register,
            Call::SchedulePosition,
        ]
    );

    host.position_requested()?;
    assert_eq!(
        calls.borrow().as_slice(),
        &[
            Call::ShellGeneration,
            Call::Register,
            Call::SchedulePosition,
            Call::ReserveAndPosition,
            Call::Show,
        ]
    );
    Ok(())
}

#[test]
fn native_invalidations_coalesce_before_the_position_message() -> Result<(), Box<dyn Error>> {
    let platform = FakePlatform::new([shell(10, 100)?]);
    let calls = platform.call_log();
    let mut host = AppBarHost::new(platform);
    host.start()?;

    host.position_invalidated()?;
    host.position_invalidated()?;

    assert_eq!(
        calls
            .borrow()
            .iter()
            .filter(|call| **call == Call::SchedulePosition)
            .count(),
        1
    );
    Ok(())
}

#[test]
fn delivered_native_invalidation_repositions_without_an_extra_message() -> Result<(), Box<dyn Error>>
{
    let platform = FakePlatform::new([shell(10, 100)?]);
    let calls = platform.call_log();
    let mut host = AppBarHost::new(platform);
    host.start()?;
    host.position_requested()?;

    host.position_event_received()?;

    let calls = calls.borrow();
    assert_eq!(
        calls
            .iter()
            .filter(|call| **call == Call::SchedulePosition)
            .count(),
        1
    );
    assert_eq!(
        calls
            .iter()
            .filter(|call| **call == Call::ReserveAndPosition)
            .count(),
        2
    );
    assert_eq!(calls.iter().filter(|call| **call == Call::Show).count(), 1);
    Ok(())
}

#[test]
fn explorer_generation_controls_reregistration_and_shutdown_removal() -> Result<(), Box<dyn Error>>
{
    let first = shell(10, 100)?;
    let replacement = shell(11, 200)?;
    let platform = FakePlatform::new([first, first, replacement]);
    let calls = platform.call_log();
    let mut host = AppBarHost::new(platform);

    host.start()?;
    host.shell_recreated()?;
    host.shell_recreated()?;
    host.shutdown()?;
    host.shutdown()?;

    assert_eq!(
        calls
            .borrow()
            .iter()
            .filter(|call| **call == Call::Register)
            .count(),
        2
    );
    assert_eq!(
        calls
            .borrow()
            .iter()
            .filter(|call| **call == Call::Remove)
            .count(),
        1
    );
    Ok(())
}
