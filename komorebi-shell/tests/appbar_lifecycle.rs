use std::error::Error;
use std::num::NonZeroU32;
use std::num::NonZeroU64;

use komorebi_shell::AppBarEdge;
use komorebi_shell::AppBarGeometry;
use komorebi_shell::AppBarLifecycle;
use komorebi_shell::PhysicalRect;
use komorebi_shell::PhysicalThickness;
use komorebi_shell::PositionCompletion;
use komorebi_shell::PositionInvalidation;
use komorebi_shell::RegistrationCompletion;
use komorebi_shell::RegistrationPlan;
use komorebi_shell::RegistrationRemoval;
use komorebi_shell::RegistrationRemovalCompletion;
use komorebi_shell::ShellGeneration;

fn shell(process_id: u32, created: u64) -> Result<ShellGeneration, Box<dyn Error>> {
    Ok(ShellGeneration::new(
        NonZeroU32::new(process_id).ok_or("process id must be nonzero")?,
        NonZeroU64::new(created).ok_or("creation time must be nonzero")?,
    ))
}

fn register(lifecycle: &mut AppBarLifecycle, shell: ShellGeneration) -> Result<(), Box<dyn Error>> {
    let RegistrationPlan::Register(attempt) = lifecycle.begin_registration(shell) else {
        return Err("expected a registration attempt".into());
    };
    assert_eq!(
        lifecycle.registration_succeeded(attempt),
        RegistrationCompletion::Registered
    );
    Ok(())
}

#[test]
fn one_shell_generation_registers_once_and_replacement_registers_again()
-> Result<(), Box<dyn Error>> {
    let first = shell(10, 100)?;
    let replacement = shell(11, 200)?;
    let mut lifecycle = AppBarLifecycle::default();

    register(&mut lifecycle, first)?;
    assert!(matches!(
        lifecycle.begin_registration(first),
        RegistrationPlan::AlreadyRegistered
    ));
    assert!(matches!(
        lifecycle.begin_registration(replacement),
        RegistrationPlan::Register(_)
    ));
    Ok(())
}

#[test]
fn native_invalidations_coalesce_and_reentrant_invalidation_schedules_one_followup()
-> Result<(), Box<dyn Error>> {
    let mut lifecycle = AppBarLifecycle::default();
    register(&mut lifecycle, shell(10, 100)?)?;

    assert_eq!(
        lifecycle.invalidate_position(),
        PositionInvalidation::Schedule
    );
    assert_eq!(
        lifecycle.invalidate_position(),
        PositionInvalidation::Coalesced
    );
    let pass = lifecycle
        .begin_position()
        .ok_or("queued position pass must begin")?;
    assert_eq!(
        lifecycle.invalidate_position(),
        PositionInvalidation::Coalesced
    );
    assert_eq!(
        lifecycle.invalidate_position(),
        PositionInvalidation::Coalesced
    );
    assert_eq!(
        lifecycle.finish_position(pass),
        PositionCompletion::ScheduleAgain
    );
    let followup = lifecycle
        .begin_position()
        .ok_or("reentrant invalidation must schedule one pass")?;
    assert_eq!(
        lifecycle.finish_position(followup),
        PositionCompletion::Settled
    );
    assert!(lifecycle.begin_position().is_none());
    Ok(())
}

#[test]
fn destroy_reports_exactly_one_native_removal_and_closes_future_registration()
-> Result<(), Box<dyn Error>> {
    let current = shell(10, 100)?;
    let mut lifecycle = AppBarLifecycle::default();
    register(&mut lifecycle, current)?;

    let RegistrationRemoval::Remove(attempt) = lifecycle.begin_destroy() else {
        return Err("registered AppBar did not request native removal".into());
    };
    assert_eq!(
        lifecycle.removal_succeeded(attempt),
        RegistrationRemovalCompletion::Destroyed
    );
    assert_eq!(lifecycle.begin_destroy(), RegistrationRemoval::Complete);
    assert!(matches!(
        lifecycle.begin_registration(current),
        RegistrationPlan::Destroyed
    ));
    Ok(())
}

#[test]
fn geometry_clamps_thickness_to_monitor_and_restores_edge_after_shell_negotiation()
-> Result<(), Box<dyn Error>> {
    let monitor = PhysicalRect::new(-100, 50, 900, 650)?;
    let geometry = AppBarGeometry::new(
        monitor,
        AppBarEdge::Right,
        PhysicalThickness::new(25).ok_or("thickness must be nonzero")?,
    );
    assert_eq!(
        geometry.proposed_rect()?,
        PhysicalRect::new(875, 50, 900, 650)?
    );

    let negotiated = PhysicalRect::new(-100, 50, 850, 650)?;
    assert_eq!(
        geometry.apply_thickness(negotiated)?,
        PhysicalRect::new(825, 50, 850, 650)?
    );

    let oversized = AppBarGeometry::new(
        monitor,
        AppBarEdge::Top,
        PhysicalThickness::new(5_000).ok_or("thickness must be nonzero")?,
    );
    assert_eq!(oversized.proposed_rect()?, monitor);
    Ok(())
}
