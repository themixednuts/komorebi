use std::num::NonZeroU64;

use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct RequestId(NonZeroU64);

impl RequestId {
    #[must_use]
    pub const fn new(value: NonZeroU64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RequestPhase {
    Pending,
    Committed,
    Terminal,
}

/// Owns the single-terminal-winner invariant for one request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestLifecycle {
    request_id: RequestId,
    phase: RequestPhase,
}

impl RequestLifecycle {
    #[must_use]
    pub const fn new(request_id: RequestId) -> Self {
        Self {
            request_id,
            phase: RequestPhase::Pending,
        }
    }

    /// Records the point after which cancellation cannot prevent execution.
    ///
    /// # Errors
    ///
    /// Returns [`RequestStateError`] if already committed or terminal.
    pub fn commit(&mut self) -> Result<(), RequestStateError> {
        match self.phase {
            RequestPhase::Pending => {
                self.phase = RequestPhase::Committed;
                Ok(())
            }
            RequestPhase::Committed => Err(RequestStateError::AlreadyCommitted),
            RequestPhase::Terminal => Err(RequestStateError::AlreadyTerminal),
        }
    }

    /// Attempts advisory cancellation and atomically chooses a terminal winner.
    #[must_use]
    pub fn cancel(&mut self) -> CancellationDecision {
        match self.phase {
            RequestPhase::Pending => {
                self.phase = RequestPhase::Terminal;
                CancellationDecision::Cancelled(TerminalWinner {
                    request_id: self.request_id,
                    phase: CompletionPhase::BeforeCommit,
                })
            }
            RequestPhase::Committed => CancellationDecision::TooLate,
            RequestPhase::Terminal => CancellationDecision::AlreadyTerminal,
        }
    }

    /// Atomically claims normal terminal completion.
    ///
    /// # Errors
    ///
    /// Returns [`RequestStateError::AlreadyTerminal`] when cancellation or a
    /// prior completion already won.
    pub fn finish(&mut self) -> Result<TerminalWinner, RequestStateError> {
        let phase = match self.phase {
            RequestPhase::Pending => CompletionPhase::BeforeCommit,
            RequestPhase::Committed => CompletionPhase::AfterCommit,
            RequestPhase::Terminal => return Err(RequestStateError::AlreadyTerminal),
        };
        self.phase = RequestPhase::Terminal;
        Ok(TerminalWinner {
            request_id: self.request_id,
            phase,
        })
    }

    #[must_use]
    pub const fn request_id(self) -> RequestId {
        self.request_id
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CancellationDecision {
    Cancelled(TerminalWinner),
    TooLate,
    AlreadyTerminal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalWinner {
    request_id: RequestId,
    phase: CompletionPhase,
}

impl TerminalWinner {
    #[must_use]
    pub const fn request_id(self) -> RequestId {
        self.request_id
    }

    #[must_use]
    pub const fn phase(self) -> CompletionPhase {
        self.phase
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompletionPhase {
    BeforeCommit,
    AfterCommit,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum RequestStateError {
    #[error("request is already committed")]
    AlreadyCommitted,
    #[error("request already has a terminal winner")]
    AlreadyTerminal,
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn request_id() -> RequestId {
        RequestId::new(NonZeroU64::MIN)
    }

    #[test]
    fn cancellation_wins_only_before_commit() -> Result<(), RequestStateError> {
        let mut pending = RequestLifecycle::new(request_id());
        assert!(matches!(
            pending.cancel(),
            CancellationDecision::Cancelled(_)
        ));
        assert_eq!(pending.finish(), Err(RequestStateError::AlreadyTerminal));

        let mut committed = RequestLifecycle::new(request_id());
        committed.commit()?;
        assert_eq!(committed.cancel(), CancellationDecision::TooLate);
        assert_eq!(committed.finish()?.phase(), CompletionPhase::AfterCommit);
        assert_eq!(committed.cancel(), CancellationDecision::AlreadyTerminal);
        Ok(())
    }

    proptest! {
        #[test]
        fn arbitrary_transition_orders_produce_at_most_one_terminal_winner(
            operations in prop::collection::vec(0_u8..3, 0..128),
        ) {
            let mut lifecycle = RequestLifecycle::new(request_id());
            let mut winners = 0;
            for operation in operations {
                match operation {
                    0 => { let _ = lifecycle.commit(); }
                    1 => {
                        if matches!(lifecycle.cancel(), CancellationDecision::Cancelled(_)) {
                            winners += 1;
                        }
                    }
                    _ => {
                        if lifecycle.finish().is_ok() {
                            winners += 1;
                        }
                    }
                }
            }
            prop_assert!(winners <= 1);
        }
    }
}
