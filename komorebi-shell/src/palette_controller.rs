use std::mem;
use std::num::NonZeroU128;

use komorebi_protocol::ActionParameter;
use komorebi_protocol::ActionUnavailability;

use crate::CommandPalette;
use crate::PaletteAction;
use crate::PaletteActionState;
use crate::PaletteCompletion;
use crate::PaletteCompletionDisposition;
use crate::PaletteEffect;
use crate::PaletteFailure;
use crate::PaletteInvocation;
use crate::PaletteMatches;
use crate::PaletteQuery;
use crate::PaletteResults;
use crate::PaletteWebInvocation;
use crate::WebSearchRequest;

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

    /// Identifies the source-specific content currently presented by a renderer.
    #[must_use]
    pub fn content(&self) -> PaletteContent<'_> {
        match &self.results {
            PaletteResults::Actions(_) => PaletteContent::Actions,
            PaletteResults::WebPrompt => PaletteContent::WebPrompt,
            PaletteResults::WebSearch(request) => PaletteContent::WebSearch(request),
        }
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
        match &self.results {
            PaletteResults::WebPrompt => None,
            PaletteResults::WebSearch(request) => {
                let request = request.clone();
                let attempt = self.begin_submission("Search the web".into())?;
                Some(PaletteEffect::Web(PaletteWebInvocation::new(
                    attempt, request,
                )))
            }
            PaletteResults::Actions(_) => self.activate_action(),
        }
    }

    fn activate_action(&mut self) -> Option<PaletteEffect> {
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
                let attempt = self.begin_submission(title)?;
                Some(PaletteEffect::Invoke(PaletteInvocation::new(
                    attempt, binding,
                )))
            }
        }
    }

    fn begin_submission(&mut self, label: Box<str>) -> Option<PaletteAttemptId> {
        let Some(attempt) = self.next_attempt else {
            self.status = PaletteStatus::AttemptIdsExhausted;
            return None;
        };
        self.next_attempt = attempt.next();
        self.status = PaletteStatus::Submitting { attempt, label };
        Some(attempt)
    }

    /// Applies a completion only when it belongs to the invocation still in flight.
    pub fn complete(&mut self, completion: PaletteCompletion) -> PaletteCompletionDisposition {
        let current = mem::replace(&mut self.status, PaletteStatus::Idle);
        match current {
            PaletteStatus::Submitting { attempt, label } if attempt == completion.attempt => {
                self.status = match completion.result {
                    Ok(()) => PaletteStatus::Succeeded { label },
                    Err(failure) => PaletteStatus::Failed { label, failure },
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

/// Source-specific palette content without renderer or adapter types.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaletteContent<'a> {
    Actions,
    WebPrompt,
    WebSearch(&'a WebSearchRequest),
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
        label: Box<str>,
    },
    Succeeded {
        label: Box<str>,
    },
    Failed {
        label: Box<str>,
        failure: PaletteFailure,
    },
    AttemptIdsExhausted,
}
