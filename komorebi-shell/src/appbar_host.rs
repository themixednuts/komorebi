use std::error::Error;

use thiserror::Error;

use crate::AppBarGeometry;
use crate::AppBarLifecycle;
use crate::PositionCompletion;
use crate::PositionInvalidation;
use crate::RegistrationCompletion;
use crate::RegistrationPlan;
use crate::RegistrationRemoval;
use crate::ShellGeneration;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppBarVisibility {
    RevealAfterPosition,
    Preserve,
}

pub trait AppBarHostPlatform {
    type Error: Error + 'static;

    fn shell_generation(&mut self) -> Result<ShellGeneration, Self::Error>;
    fn register(&mut self) -> Result<(), Self::Error>;
    fn schedule_position(&mut self) -> Result<(), Self::Error>;
    fn position(&mut self, visibility: AppBarVisibility) -> Result<(), Self::Error>;
    fn remove(&mut self) -> Result<(), Self::Error>;
    fn update_geometry(&mut self, geometry: AppBarGeometry);
}

#[must_use = "an AppBar host must be shut down to release its native reservation"]
pub struct AppBarHost<P> {
    platform: P,
    lifecycle: AppBarLifecycle,
    visible: bool,
}

impl<P> AppBarHost<P>
where
    P: AppBarHostPlatform,
{
    pub fn new(platform: P) -> Self {
        Self {
            platform,
            lifecycle: AppBarLifecycle::default(),
            visible: false,
        }
    }

    pub fn start(&mut self) -> Result<(), AppBarHostError<P::Error>> {
        let shell = self
            .platform
            .shell_generation()
            .map_err(AppBarHostError::ShellGeneration)?;
        match self.lifecycle.begin_registration(shell) {
            RegistrationPlan::Register(attempt) => match self.platform.register() {
                Ok(()) => {
                    if self.lifecycle.registration_succeeded(attempt)
                        == RegistrationCompletion::Registered
                    {
                        self.request_position()?;
                    }
                    Ok(())
                }
                Err(error) => {
                    self.lifecycle.registration_failed(attempt);
                    Err(AppBarHostError::Registration(error))
                }
            },
            RegistrationPlan::AlreadyRegistered => Ok(()),
            RegistrationPlan::Destroyed => Err(AppBarHostError::Destroyed),
        }
    }

    pub fn shell_recreated(&mut self) -> Result<(), AppBarHostError<P::Error>> {
        self.start()
    }

    pub fn position_invalidated(&mut self) -> Result<(), AppBarHostError<P::Error>> {
        self.request_position()
    }

    pub fn geometry_changed(
        &mut self,
        geometry: AppBarGeometry,
    ) -> Result<(), AppBarHostError<P::Error>> {
        self.platform.update_geometry(geometry);
        self.request_position()
    }

    pub fn position_requested(&mut self) -> Result<(), AppBarHostError<P::Error>> {
        let Some(pass) = self.lifecycle.begin_position() else {
            return Ok(());
        };
        let visibility = if self.visible {
            AppBarVisibility::Preserve
        } else {
            AppBarVisibility::RevealAfterPosition
        };
        let position = self.platform.position(visibility);
        let completion = self.lifecycle.finish_position(pass);
        if position.is_ok() {
            self.visible = true;
        }
        let reschedule = match completion {
            PositionCompletion::ScheduleAgain => self.platform.schedule_position(),
            PositionCompletion::Settled | PositionCompletion::Stale => Ok(()),
        };
        match (position, reschedule) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(position), Ok(())) => Err(AppBarHostError::Position(position)),
            (Ok(()), Err(schedule)) => Err(AppBarHostError::PositionSchedule(schedule)),
            (Err(position), Err(schedule)) => {
                Err(AppBarHostError::PositionAndReschedule { position, schedule })
            }
        }
    }

    pub fn shutdown(&mut self) -> Result<(), AppBarHostError<P::Error>> {
        if let RegistrationRemoval::Remove(attempt) = self.lifecycle.begin_destroy() {
            match self.platform.remove() {
                Ok(()) => {
                    self.lifecycle.removal_succeeded(attempt);
                }
                Err(error) => {
                    self.lifecycle.removal_failed(attempt);
                    return Err(AppBarHostError::Removal(error));
                }
            }
        }
        Ok(())
    }

    fn request_position(&mut self) -> Result<(), AppBarHostError<P::Error>> {
        match self.lifecycle.invalidate_position() {
            PositionInvalidation::Schedule => match self.platform.schedule_position() {
                Ok(()) => Ok(()),
                Err(error) => {
                    self.lifecycle.position_scheduling_failed();
                    Err(AppBarHostError::PositionSchedule(error))
                }
            },
            PositionInvalidation::Coalesced => Ok(()),
            PositionInvalidation::Destroyed => Err(AppBarHostError::Destroyed),
        }
    }
}

#[derive(Debug, Error)]
pub enum AppBarHostError<E>
where
    E: Error + 'static,
{
    #[error("could not identify the current Windows shell generation")]
    ShellGeneration(#[source] E),
    #[error("could not register the AppBar")]
    Registration(#[source] E),
    #[error("could not schedule AppBar positioning")]
    PositionSchedule(#[source] E),
    #[error("could not position the AppBar")]
    Position(#[source] E),
    #[error("could not position or reschedule the AppBar")]
    PositionAndReschedule {
        #[source]
        position: E,
        schedule: E,
    },
    #[error("could not remove the AppBar registration")]
    Removal(#[source] E),
    #[error("the AppBar host has already been destroyed")]
    Destroyed,
}
