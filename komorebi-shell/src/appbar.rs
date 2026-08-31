use std::num::NonZeroU32;
use std::num::NonZeroU64;

use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppBarEdge {
    Left,
    Top,
    Right,
    Bottom,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhysicalRect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
    width: NonZeroU32,
    height: NonZeroU32,
}

impl PhysicalRect {
    pub fn new(left: i32, top: i32, right: i32, bottom: i32) -> Result<Self, PhysicalRectError> {
        let width = u32::try_from(i64::from(right) - i64::from(left))
            .ok()
            .and_then(NonZeroU32::new)
            .ok_or(PhysicalRectError::EmptyWidth { left, right })?;
        let height = u32::try_from(i64::from(bottom) - i64::from(top))
            .ok()
            .and_then(NonZeroU32::new)
            .ok_or(PhysicalRectError::EmptyHeight { top, bottom })?;
        Ok(Self {
            left,
            top,
            right,
            bottom,
            width,
            height,
        })
    }

    #[must_use]
    pub const fn left(self) -> i32 {
        self.left
    }

    #[must_use]
    pub const fn top(self) -> i32 {
        self.top
    }

    #[must_use]
    pub const fn right(self) -> i32 {
        self.right
    }

    #[must_use]
    pub const fn bottom(self) -> i32 {
        self.bottom
    }

    const fn width(self) -> u32 {
        self.width.get()
    }

    const fn height(self) -> u32 {
        self.height.get()
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum PhysicalRectError {
    #[error("physical rectangle has no width: left {left}, right {right}")]
    EmptyWidth { left: i32, right: i32 },
    #[error("physical rectangle has no height: top {top}, bottom {bottom}")]
    EmptyHeight { top: i32, bottom: i32 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhysicalThickness(NonZeroU32);

impl PhysicalThickness {
    #[must_use]
    pub const fn new(value: u32) -> Option<Self> {
        match NonZeroU32::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    const fn get(self) -> u32 {
        self.0.get()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AppBarGeometry {
    monitor: PhysicalRect,
    edge: AppBarEdge,
    thickness: PhysicalThickness,
}

impl AppBarGeometry {
    #[must_use]
    pub const fn new(
        monitor: PhysicalRect,
        edge: AppBarEdge,
        thickness: PhysicalThickness,
    ) -> Self {
        Self {
            monitor,
            edge,
            thickness,
        }
    }

    pub fn proposed_rect(self) -> Result<PhysicalRect, PhysicalRectError> {
        self.apply_thickness(self.monitor)
    }

    pub fn apply_thickness(
        self,
        negotiated: PhysicalRect,
    ) -> Result<PhysicalRect, PhysicalRectError> {
        let axis_span = match self.edge {
            AppBarEdge::Left | AppBarEdge::Right => negotiated.width(),
            AppBarEdge::Top | AppBarEdge::Bottom => negotiated.height(),
        };
        let thickness = self.thickness.get().min(axis_span);
        let thickness = i64::from(thickness);
        match self.edge {
            AppBarEdge::Left => PhysicalRect::new(
                negotiated.left,
                negotiated.top,
                coordinate(i64::from(negotiated.left) + thickness),
                negotiated.bottom,
            ),
            AppBarEdge::Top => PhysicalRect::new(
                negotiated.left,
                negotiated.top,
                negotiated.right,
                coordinate(i64::from(negotiated.top) + thickness),
            ),
            AppBarEdge::Right => PhysicalRect::new(
                coordinate(i64::from(negotiated.right) - thickness),
                negotiated.top,
                negotiated.right,
                negotiated.bottom,
            ),
            AppBarEdge::Bottom => PhysicalRect::new(
                negotiated.left,
                coordinate(i64::from(negotiated.bottom) - thickness),
                negotiated.right,
                negotiated.bottom,
            ),
        }
    }
}

fn coordinate(value: i64) -> i32 {
    match i32::try_from(value) {
        Ok(value) => value,
        Err(_) if value.is_negative() => i32::MIN,
        Err(_) => i32::MAX,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellGeneration {
    process_id: NonZeroU32,
    created_100ns: NonZeroU64,
}

impl ShellGeneration {
    #[must_use]
    pub const fn new(process_id: NonZeroU32, created_100ns: NonZeroU64) -> Self {
        Self {
            process_id,
            created_100ns,
        }
    }

    #[must_use]
    pub const fn process_id(self) -> NonZeroU32 {
        self.process_id
    }

    #[must_use]
    pub const fn created_100ns(self) -> NonZeroU64 {
        self.created_100ns
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum RegistrationPlan {
    Register(RegistrationAttempt),
    AlreadyRegistered,
    Destroyed,
}

#[derive(Debug, Eq, PartialEq)]
pub struct RegistrationAttempt {
    shell: ShellGeneration,
}

impl RegistrationAttempt {
    #[must_use]
    pub const fn shell(&self) -> ShellGeneration {
        self.shell
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegistrationCompletion {
    Registered,
    Stale,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegistrationRemoval {
    Remove,
    None,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PositionInvalidation {
    Schedule,
    Coalesced,
    Destroyed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PositionCompletion {
    Settled,
    ScheduleAgain,
    Stale,
}

#[derive(Debug)]
pub struct PositionPass {
    _private: (),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Registration {
    Detached,
    Registering(ShellGeneration),
    Registered(ShellGeneration),
    Destroyed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Position {
    Settled,
    Queued,
    Applying { invalidated: bool },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AppBarLifecycle {
    registration: Registration,
    position: Position,
}

impl Default for AppBarLifecycle {
    fn default() -> Self {
        Self {
            registration: Registration::Detached,
            position: Position::Settled,
        }
    }
}

impl AppBarLifecycle {
    pub fn begin_registration(&mut self, shell: ShellGeneration) -> RegistrationPlan {
        match self.registration {
            Registration::Registered(current) | Registration::Registering(current)
                if current == shell =>
            {
                RegistrationPlan::AlreadyRegistered
            }
            Registration::Destroyed => RegistrationPlan::Destroyed,
            Registration::Detached | Registration::Registering(_) | Registration::Registered(_) => {
                self.registration = Registration::Registering(shell);
                RegistrationPlan::Register(RegistrationAttempt { shell })
            }
        }
    }

    #[allow(
        clippy::needless_pass_by_value,
        reason = "consuming the proof token prevents two native completion attempts"
    )]
    pub fn registration_succeeded(
        &mut self,
        attempt: RegistrationAttempt,
    ) -> RegistrationCompletion {
        let RegistrationAttempt { shell } = attempt;
        if self.registration == Registration::Registering(shell) {
            self.registration = Registration::Registered(shell);
            RegistrationCompletion::Registered
        } else {
            RegistrationCompletion::Stale
        }
    }

    #[allow(
        clippy::needless_pass_by_value,
        reason = "consuming the proof token closes the registration attempt"
    )]
    pub fn registration_failed(&mut self, attempt: RegistrationAttempt) {
        let RegistrationAttempt { shell } = attempt;
        if self.registration == Registration::Registering(shell) {
            self.registration = Registration::Detached;
        }
    }

    pub fn invalidate_position(&mut self) -> PositionInvalidation {
        match self.registration {
            Registration::Destroyed => PositionInvalidation::Destroyed,
            Registration::Detached | Registration::Registering(_) | Registration::Registered(_) => {
                match self.position {
                    Position::Settled => {
                        self.position = Position::Queued;
                        PositionInvalidation::Schedule
                    }
                    Position::Queued => PositionInvalidation::Coalesced,
                    Position::Applying { .. } => {
                        self.position = Position::Applying { invalidated: true };
                        PositionInvalidation::Coalesced
                    }
                }
            }
        }
    }

    pub fn begin_position(&mut self) -> Option<PositionPass> {
        if !matches!(self.registration, Registration::Registered(_))
            || self.position != Position::Queued
        {
            return None;
        }
        self.position = Position::Applying { invalidated: false };
        Some(PositionPass { _private: () })
    }

    #[allow(
        clippy::needless_pass_by_value,
        reason = "consuming the proof token makes each position pass finish once"
    )]
    pub fn finish_position(&mut self, pass: PositionPass) -> PositionCompletion {
        let PositionPass { _private: () } = pass;
        match self.position {
            Position::Applying { invalidated: true } => {
                self.position = Position::Queued;
                PositionCompletion::ScheduleAgain
            }
            Position::Applying { invalidated: false } => {
                self.position = Position::Settled;
                PositionCompletion::Settled
            }
            Position::Settled | Position::Queued => PositionCompletion::Stale,
        }
    }

    pub fn detach(&mut self) -> RegistrationRemoval {
        let removal = if matches!(self.registration, Registration::Registered(_)) {
            RegistrationRemoval::Remove
        } else {
            RegistrationRemoval::None
        };
        if self.registration != Registration::Destroyed {
            self.registration = Registration::Detached;
            self.position = Position::Settled;
        }
        removal
    }

    pub fn destroy(&mut self) -> RegistrationRemoval {
        let removal = self.detach();
        self.registration = Registration::Destroyed;
        removal
    }
}
