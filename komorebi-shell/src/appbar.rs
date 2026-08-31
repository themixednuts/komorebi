use std::num::NonZeroU32;
use std::num::NonZeroU64;

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

#[derive(Debug, Eq, PartialEq)]
pub enum RegistrationRemoval {
    Remove(RegistrationRemovalAttempt),
    Complete,
    InProgress,
}

#[derive(Debug, Eq, PartialEq)]
pub struct RegistrationRemovalAttempt {
    shell: ShellGeneration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegistrationRemovalCompletion {
    Destroyed,
    Stale,
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
    Removing(ShellGeneration),
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
            Registration::Registered(current)
            | Registration::Registering(current)
            | Registration::Removing(current)
                if current == shell =>
            {
                RegistrationPlan::AlreadyRegistered
            }
            Registration::Destroyed => RegistrationPlan::Destroyed,
            Registration::Detached
            | Registration::Registering(_)
            | Registration::Registered(_)
            | Registration::Removing(_) => {
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
            Registration::Detached
            | Registration::Registering(_)
            | Registration::Registered(_)
            | Registration::Removing(_) => match self.position {
                Position::Settled => {
                    self.position = Position::Queued;
                    PositionInvalidation::Schedule
                }
                Position::Queued => PositionInvalidation::Coalesced,
                Position::Applying { .. } => {
                    self.position = Position::Applying { invalidated: true };
                    PositionInvalidation::Coalesced
                }
            },
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

    pub(crate) fn position_scheduling_failed(&mut self) {
        if self.position == Position::Queued {
            self.position = Position::Settled;
        }
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

    pub fn begin_destroy(&mut self) -> RegistrationRemoval {
        self.position = Position::Settled;
        match self.registration {
            Registration::Registered(shell) => {
                self.registration = Registration::Removing(shell);
                RegistrationRemoval::Remove(RegistrationRemovalAttempt { shell })
            }
            Registration::Removing(_) => RegistrationRemoval::InProgress,
            Registration::Detached | Registration::Registering(_) | Registration::Destroyed => {
                self.registration = Registration::Destroyed;
                RegistrationRemoval::Complete
            }
        }
    }

    #[allow(
        clippy::needless_pass_by_value,
        reason = "consuming the proof token prevents duplicate removal completion"
    )]
    pub fn removal_succeeded(
        &mut self,
        attempt: RegistrationRemovalAttempt,
    ) -> RegistrationRemovalCompletion {
        let RegistrationRemovalAttempt { shell } = attempt;
        if self.registration == Registration::Removing(shell) {
            self.registration = Registration::Destroyed;
            RegistrationRemovalCompletion::Destroyed
        } else {
            RegistrationRemovalCompletion::Stale
        }
    }

    #[allow(
        clippy::needless_pass_by_value,
        reason = "consuming the proof token closes the failed removal attempt"
    )]
    pub fn removal_failed(&mut self, attempt: RegistrationRemovalAttempt) {
        let RegistrationRemovalAttempt { shell } = attempt;
        if self.registration == Registration::Removing(shell) {
            self.registration = Registration::Registered(shell);
        }
    }
}
