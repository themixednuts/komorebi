use komorebi_protocol::InvocationRejection;
use komorebi_protocol::InvocationSubmissionReply;

use crate::ActionBinding;
use crate::ActionInvocationError;
use crate::InvocationTicket;
use crate::PaletteAttemptId;
use crate::ShellHandle;
use crate::ShellRequestError;
use crate::WebActivationCompletionError;
use crate::WebActivationSubmitError;
use crate::WebActivationTicket;
use crate::WebLaunchDisposition;
use crate::WebLaunchFailure;
use crate::WebSearchBroker;
use crate::WebSearchRequest;

/// The terminal completion of one palette invocation attempt.
#[derive(Debug)]
pub struct PaletteCompletion {
    pub(crate) attempt: PaletteAttemptId,
    pub(crate) result: Result<(), PaletteFailure>,
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

/// A typed terminal failure produced by a palette effect adapter.
#[derive(Debug)]
pub enum PaletteFailure {
    Submission(ShellRequestError),
    Rejected(InvocationRejection),
    Execution(ActionInvocationError),
    WebSubmission(WebActivationSubmitError),
    WebCompletion(WebActivationCompletionError),
    WebLaunch(WebLaunchFailure),
    WebRejected,
    WebUnavailable,
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
    Web(PaletteWebInvocation),
}

/// One authorized action invocation carrying its stale-completion fence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaletteInvocation {
    attempt: PaletteAttemptId,
    binding: ActionBinding,
}

impl PaletteInvocation {
    pub(crate) const fn new(attempt: PaletteAttemptId, binding: ActionBinding) -> Self {
        Self { attempt, binding }
    }

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
                effect: PendingPaletteEffect::Action(ticket),
            }),
            Err(error) => PaletteSubmission::Complete(PaletteCompletion::failed(
                self.attempt,
                PaletteFailure::Submission(error),
            )),
        }
    }
}

/// One brokered web-search invocation carrying its stale-completion fence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaletteWebInvocation {
    attempt: PaletteAttemptId,
    request: WebSearchRequest,
}

impl PaletteWebInvocation {
    pub(crate) const fn new(attempt: PaletteAttemptId, request: WebSearchRequest) -> Self {
        Self { attempt, request }
    }

    #[must_use]
    pub const fn attempt(&self) -> PaletteAttemptId {
        self.attempt
    }

    #[must_use]
    pub const fn request(&self) -> &WebSearchRequest {
        &self.request
    }

    /// Submits through the owned web broker without exposing native URI logic.
    pub async fn submit(self, web: &WebSearchBroker) -> PaletteSubmission {
        let WebSearchBroker::Configured(web) = web else {
            return PaletteSubmission::Complete(PaletteCompletion::failed(
                self.attempt,
                PaletteFailure::WebUnavailable,
            ));
        };
        match web.submit(self.request).await {
            Ok(ticket) => PaletteSubmission::Pending(PendingPaletteInvocation {
                attempt: self.attempt,
                effect: PendingPaletteEffect::Web(ticket),
            }),
            Err(error) => PaletteSubmission::Complete(PaletteCompletion::failed(
                self.attempt,
                PaletteFailure::WebSubmission(error),
            )),
        }
    }
}

/// The immediate result of handing an invocation to its owned effect service.
pub enum PaletteSubmission {
    Pending(PendingPaletteInvocation),
    Complete(PaletteCompletion),
}

impl PaletteSubmission {
    /// Resolves immediate and pending submissions through one completion path.
    pub async fn complete(self) -> PaletteCompletion {
        match self {
            Self::Pending(pending) => pending.complete().await,
            Self::Complete(completion) => completion,
        }
    }
}

/// An owned wait for one palette effect whose service retains admitted work.
pub struct PendingPaletteInvocation {
    attempt: PaletteAttemptId,
    effect: PendingPaletteEffect,
}

enum PendingPaletteEffect {
    Action(InvocationTicket),
    Web(WebActivationTicket),
}

impl PendingPaletteInvocation {
    /// Waits for the actor-owned operation and translates its terminal result.
    pub async fn complete(self) -> PaletteCompletion {
        let result = match self.effect {
            PendingPaletteEffect::Action(ticket) => match ticket.outcome().await {
                Ok(
                    InvocationSubmissionReply::Accepted(_) | InvocationSubmissionReply::Retained(_),
                ) => Ok(()),
                Ok(InvocationSubmissionReply::Rejected(reason)) => {
                    Err(PaletteFailure::Rejected(reason))
                }
                Err(error) => Err(PaletteFailure::Execution(error)),
            },
            PendingPaletteEffect::Web(ticket) => match ticket.complete().await {
                Ok(Ok(WebLaunchDisposition::Launched)) => Ok(()),
                Ok(Ok(WebLaunchDisposition::Rejected)) => Err(PaletteFailure::WebRejected),
                Ok(Err(error)) => Err(PaletteFailure::WebLaunch(error)),
                Err(error) => Err(PaletteFailure::WebCompletion(error)),
            },
        };
        PaletteCompletion {
            attempt: self.attempt,
            result,
        }
    }
}
