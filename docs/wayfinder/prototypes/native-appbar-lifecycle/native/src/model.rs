use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Edge {
    Left,
    Top,
    Right,
    Bottom,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    #[must_use]
    pub fn width(self) -> i32 {
        self.right.saturating_sub(self.left)
    }

    #[must_use]
    pub fn height(self) -> i32 {
        self.bottom.saturating_sub(self.top)
    }

    #[must_use]
    pub fn overlaps(self, other: Self) -> bool {
        self.left < other.right
            && other.left < self.right
            && self.top < other.bottom
            && other.top < self.bottom
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AppBarGeometry {
    pub monitor: Rect,
    pub edge: Edge,
    pub thickness: i32,
}

impl AppBarGeometry {
    #[must_use]
    pub fn proposed_rect(self) -> Rect {
        let thickness = self.thickness.max(1).min(match self.edge {
            Edge::Left | Edge::Right => self.monitor.width(),
            Edge::Top | Edge::Bottom => self.monitor.height(),
        });

        match self.edge {
            Edge::Left => Rect {
                right: self.monitor.left.saturating_add(thickness),
                ..self.monitor
            },
            Edge::Top => Rect {
                bottom: self.monitor.top.saturating_add(thickness),
                ..self.monitor
            },
            Edge::Right => Rect {
                left: self.monitor.right.saturating_sub(thickness),
                ..self.monitor
            },
            Edge::Bottom => Rect {
                top: self.monitor.bottom.saturating_sub(thickness),
                ..self.monitor
            },
        }
    }

    #[must_use]
    pub fn apply_thickness(self, negotiated: Rect) -> Rect {
        let thickness = self.proposed_rect();
        match self.edge {
            Edge::Left => Rect {
                right: negotiated.left.saturating_add(thickness.width()),
                ..negotiated
            },
            Edge::Top => Rect {
                bottom: negotiated.top.saturating_add(thickness.height()),
                ..negotiated
            },
            Edge::Right => Rect {
                left: negotiated.right.saturating_sub(thickness.width()),
                ..negotiated
            },
            Edge::Bottom => Rect {
                top: negotiated.bottom.saturating_sub(thickness.height()),
                ..negotiated
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ShellIdentity {
    pub process_id: u32,
    pub created_100ns: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Registration {
    Detached,
    Registering(ShellIdentity),
    Registered(ShellIdentity),
    Destroyed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Position {
    Settled,
    Queued,
    Applying { invalidated: bool },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegisterDecision {
    Register,
    AlreadyRegistered,
    Destroyed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Lifecycle {
    registration: Registration,
    position: Position,
}

impl Default for Lifecycle {
    fn default() -> Self {
        Self {
            registration: Registration::Detached,
            position: Position::Settled,
        }
    }
}

impl Lifecycle {
    pub fn begin_registration(&mut self, shell: ShellIdentity) -> RegisterDecision {
        match self.registration {
            Registration::Registered(current) | Registration::Registering(current)
                if current == shell =>
            {
                RegisterDecision::AlreadyRegistered
            }
            Registration::Destroyed => RegisterDecision::Destroyed,
            Registration::Detached | Registration::Registering(_) | Registration::Registered(_) => {
                self.registration = Registration::Registering(shell);
                RegisterDecision::Register
            }
        }
    }

    pub fn registration_succeeded(&mut self, shell: ShellIdentity) -> bool {
        if self.registration != Registration::Registering(shell) {
            return false;
        }

        self.registration = Registration::Registered(shell);
        true
    }

    pub fn registration_failed(&mut self, shell: ShellIdentity) {
        if self.registration == Registration::Registering(shell) {
            self.registration = Registration::Detached;
        }
    }

    #[must_use]
    pub fn registered_shell(self) -> Option<ShellIdentity> {
        match self.registration {
            Registration::Registered(shell) => Some(shell),
            Registration::Detached | Registration::Registering(_) | Registration::Destroyed => None,
        }
    }

    pub fn request_position(&mut self) -> bool {
        match self.position {
            Position::Settled => {
                self.position = Position::Queued;
                true
            }
            Position::Queued => false,
            Position::Applying { .. } => {
                self.position = Position::Applying { invalidated: true };
                false
            }
        }
    }

    pub fn begin_position(&mut self) -> bool {
        if self.position != Position::Queued || self.registered_shell().is_none() {
            return false;
        }

        self.position = Position::Applying { invalidated: false };
        true
    }

    pub fn finish_position(&mut self) -> bool {
        match self.position {
            Position::Applying { invalidated: true } => {
                self.position = Position::Queued;
                true
            }
            Position::Applying { invalidated: false } => {
                self.position = Position::Settled;
                false
            }
            Position::Settled | Position::Queued => false,
        }
    }

    pub fn detach(&mut self) -> bool {
        let was_registered = matches!(self.registration, Registration::Registered(_));
        if self.registration != Registration::Destroyed {
            self.registration = Registration::Detached;
            self.position = Position::Settled;
        }
        was_registered
    }

    pub fn destroy(&mut self) -> bool {
        let was_registered = self.detach();
        self.registration = Registration::Destroyed;
        was_registered
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHELL_A: ShellIdentity = ShellIdentity {
        process_id: 10,
        created_100ns: 100,
    };
    const SHELL_B: ShellIdentity = ShellIdentity {
        process_id: 11,
        created_100ns: 200,
    };

    #[test]
    fn duplicate_registration_for_one_shell_is_suppressed() {
        let mut lifecycle = Lifecycle::default();
        assert_eq!(
            lifecycle.begin_registration(SHELL_A),
            RegisterDecision::Register
        );
        assert!(lifecycle.registration_succeeded(SHELL_A));
        assert_eq!(
            lifecycle.begin_registration(SHELL_A),
            RegisterDecision::AlreadyRegistered
        );
    }

    #[test]
    fn new_shell_identity_requires_registration() {
        let mut lifecycle = Lifecycle::default();
        assert_eq!(
            lifecycle.begin_registration(SHELL_A),
            RegisterDecision::Register
        );
        assert!(lifecycle.registration_succeeded(SHELL_A));
        assert_eq!(
            lifecycle.begin_registration(SHELL_B),
            RegisterDecision::Register
        );
    }

    #[test]
    fn invalidations_coalesce_and_reentrant_invalidation_queues_one_pass() {
        let mut lifecycle = Lifecycle::default();
        assert_eq!(
            lifecycle.begin_registration(SHELL_A),
            RegisterDecision::Register
        );
        assert!(lifecycle.registration_succeeded(SHELL_A));
        assert!(lifecycle.request_position());
        assert!(!lifecycle.request_position());
        assert!(lifecycle.begin_position());
        assert!(!lifecycle.request_position());
        assert!(!lifecycle.request_position());
        assert!(lifecycle.finish_position());
        assert!(lifecycle.begin_position());
        assert!(!lifecycle.finish_position());
    }

    #[test]
    fn right_edge_thickness_is_restored_after_shell_adjustment() {
        let geometry = AppBarGeometry {
            monitor: Rect {
                left: -100,
                top: 50,
                right: 900,
                bottom: 650,
            },
            edge: Edge::Right,
            thickness: 25,
        };
        let adjusted = geometry.apply_thickness(Rect {
            left: -100,
            top: 50,
            right: 850,
            bottom: 650,
        });
        assert_eq!(adjusted.left, 825);
        assert_eq!(adjusted.right, 850);
    }
}
