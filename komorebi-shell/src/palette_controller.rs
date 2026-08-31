use std::mem;
use std::num::NonZeroU128;

use komorebi_protocol::ActionParameter;
use komorebi_protocol::ActionUnavailability;
use komorebi_protocol::InvocationRejection;
use komorebi_protocol::InvocationSubmissionReply;

use crate::ActionBinding;
use crate::ActionInvocationError;
use crate::CommandPalette;
use crate::InvocationTicket;
use crate::PaletteAction;
use crate::PaletteActionState;
use crate::PaletteMatches;
use crate::PaletteQuery;
use crate::PaletteResults;
use crate::ShellHandle;
use crate::ShellRequestError;

/// Renderer-neutral state and transitions for one command-palette surface.
#[derive(Debug)]
pub struct PaletteController {
    palette: CommandPalette,
    results: PaletteResults,
    status: PaletteStatus,
    next_attempt: Option<PaletteAttemptId>,
}

impl PaletteController {
    #[must_use]
    pub fn new(palette: CommandPalette) -> Self {
        let results = palette.query(PaletteQuery::Browse);
        Self {
            palette,
            results,
            status: PaletteStatus::Idle,
            next_attempt: Some(PaletteAttemptId::FIRST),
        }
    }

    /// Replaces visible results while preserving an invocation already in flight.
    pub fn update_query(&mut self, input: &str) {
        self.results = self.palette.query(PaletteQuery::parse(input));
        if !matches!(self.status, PaletteStatus::Submitting { .. }) {
            self.status = PaletteStatus::Idle;
        }
    }

    #[must_use]
    pub fn selected_action(&self) -> Option<&PaletteAction> {
        self.matches()
            .and_then(|matches| matches.selected(&self.palette))
    }

    /// Iterates the currently visible manager-action rows.
    pub fn actions(&self) -> impl Iterator<Item = &PaletteAction> {
        self.matches()
            .into_iter()
            .flat_map(|matches| matches.actions(&self.palette))
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.matches().is_none_or(PaletteMatches::is_empty)
    }

    #[must_use]
    pub fn selected_position(&self) -> Option<usize> {
        self.matches().and_then(PaletteMatches::selected_position)
    }

    /// Selects one visible result, returning whether the position exists.
    pub fn select_position(&mut self, position: usize) -> bool {
        self.matches_mut()
            .is_some_and(|matches| matches.select_position(position))
    }

    pub fn move_selection(&mut self, movement: crate::PaletteSelectionMove) {
        if let Some(matches) = self.matches_mut() {
            matches.move_selection(movement);
        }
    }

    #[must_use]
    pub const fn status(&self) -> &PaletteStatus {
        &self.status
    }

    /// Begins the selected effect exactly once while an action is in flight.
    #[must_use]
    pub fn activate(&mut self) -> Option<PaletteEffect> {
        if matches!(self.status, PaletteStatus::Submitting { .. }) {
            return None;
        }
        let action = self.selected_action()?;
        let title = action.title().into();
        match action.state() {
            PaletteActionState::Unavailable(reason) => {
                self.status = PaletteStatus::Unavailable {
                    action: title,
                    reason,
                };
                None
            }
            PaletteActionState::RequiresInput(parameters) => {
                self.status = PaletteStatus::RequiresInput {
                    action: title,
                    parameters: parameters.into(),
                };
                None
            }
            PaletteActionState::Ready(binding) => {
                let Some(attempt) = self.next_attempt else {
                    self.status = PaletteStatus::AttemptIdsExhausted;
                    return None;
                };
                self.next_attempt = attempt.next();
                self.status = PaletteStatus::Submitting {
                    attempt,
                    action: title,
                };
                Some(PaletteEffect::Invoke(PaletteInvocation {
                    attempt,
                    binding,
                }))
            }
        }
    }

    /// Applies a completion only when it belongs to the invocation still in flight.
    pub fn complete(&mut self, completion: PaletteCompletion) -> PaletteCompletionDisposition {
        let current = mem::replace(&mut self.status, PaletteStatus::Idle);
        match current {
            PaletteStatus::Submitting { attempt, action } if attempt == completion.attempt => {
                self.status = match completion.result {
                    Ok(()) => PaletteStatus::Succeeded { action },
                    Err(failure) => PaletteStatus::Failed { action, failure },
                };
                PaletteCompletionDisposition::Applied
            }
            current => {
                self.status = current;
                PaletteCompletionDisposition::IgnoredStale
            }
        }
    }

    fn matches(&self) -> Option<&PaletteMatches> {
        match &self.results {
            PaletteResults::Actions(matches) => Some(matches),
            PaletteResults::WebPrompt | PaletteResults::WebSearch(_) => None,
        }
    }

    fn matches_mut(&mut self) -> Option<&mut PaletteMatches> {
        match &mut self.results {
            PaletteResults::Actions(matches) => Some(matches),
            PaletteResults::WebPrompt | PaletteResults::WebSearch(_) => None,
        }
    }
}

/// A unique activation attempt within one palette-controller lifetime.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PaletteAttemptId(NonZeroU128);

impl PaletteAttemptId {
    const FIRST: Self = Self(NonZeroU128::MIN);

    fn next(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }
}

/// The visible activation state shared by every palette renderer.
#[derive(Debug)]
pub enum PaletteStatus {
    Idle,
    RequiresInput {
        action: Box<str>,
        parameters: Box<[ActionParameter]>,
    },
    Unavailable {
        action: Box<str>,
        reason: ActionUnavailability,
    },
    Submitting {
        attempt: PaletteAttemptId,
        action: Box<str>,
    },
    Succeeded {
        action: Box<str>,
    },
    Failed {
        action: Box<str>,
        failure: PaletteFailure,
    },
    AttemptIdsExhausted,
}

/// The terminal completion of one palette invocation attempt.
#[derive(Debug)]
pub struct PaletteCompletion {
    attempt: PaletteAttemptId,
    result: Result<(), PaletteFailure>,
}

impl PaletteCompletion {
    #[must_use]
    pub const fn succeeded(attempt: PaletteAttemptId) -> Self {
        Self {
            attempt,
            result: Ok(()),
        }
    }

    const fn failed(attempt: PaletteAttemptId, failure: PaletteFailure) -> Self {
        Self {
            attempt,
            result: Err(failure),
        }
    }
}

/// A typed terminal failure produced by the shell invocation adapter.
#[derive(Debug)]
pub enum PaletteFailure {
    Submission(ShellRequestError),
    Rejected(InvocationRejection),
    Execution(ActionInvocationError),
}

/// Whether a completion changed the currently visible controller state.
#[must_use]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaletteCompletionDisposition {
    Applied,
    IgnoredStale,
}

/// A side effect selected by a pure controller transition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PaletteEffect {
    Invoke(PaletteInvocation),
}

/// One authorized action invocation carrying its stale-completion fence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaletteInvocation {
    attempt: PaletteAttemptId,
    binding: ActionBinding,
}

impl PaletteInvocation {
    #[must_use]
    pub const fn attempt(&self) -> PaletteAttemptId {
        self.attempt
    }

    #[must_use]
    pub const fn binding(&self) -> &ActionBinding {
        &self.binding
    }

    /// Submits through the owned shell session without exposing transport logic to a renderer.
    #[must_use]
    pub fn submit(self, shell: &ShellHandle) -> PaletteSubmission {
        match shell.invoke_binding(self.binding) {
            Ok(ticket) => PaletteSubmission::Pending(PendingPaletteInvocation {
                attempt: self.attempt,
                ticket,
            }),
            Err(error) => PaletteSubmission::Complete(PaletteCompletion::failed(
                self.attempt,
                PaletteFailure::Submission(error),
            )),
        }
    }
}

/// The immediate result of handing an invocation to the owned shell session.
pub enum PaletteSubmission {
    Pending(PendingPaletteInvocation),
    Complete(PaletteCompletion),
}

/// An owned wait for one action submission that remains cancellation-safe in `ShellSession`.
pub struct PendingPaletteInvocation {
    attempt: PaletteAttemptId,
    ticket: InvocationTicket,
}

impl PendingPaletteInvocation {
    /// Waits for the actor-owned operation and translates its terminal result.
    pub async fn complete(self) -> PaletteCompletion {
        let result = match self.ticket.outcome().await {
            Ok(InvocationSubmissionReply::Accepted(_) | InvocationSubmissionReply::Retained(_)) => {
                Ok(())
            }
            Ok(InvocationSubmissionReply::Rejected(reason)) => {
                Err(PaletteFailure::Rejected(reason))
            }
            Err(error) => Err(PaletteFailure::Execution(error)),
        };
        PaletteCompletion {
            attempt: self.attempt,
            result,
        }
    }
}
