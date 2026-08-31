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
use crate::PaletteFileInvocation;
use crate::PaletteFileSearch;
use crate::PaletteFileSearchCompletion;
use crate::PaletteFileSearchCompletionDisposition;
use crate::PaletteFileSearchFailure;
use crate::PaletteInvocation;
use crate::PaletteMatches;
use crate::PaletteQuery;
use crate::PaletteQueryRevision;
use crate::PaletteResults;
use crate::PaletteWebInvocation;
use crate::WebSearchRequest;
use komorebi_search::FileSearchMatch;

/// Renderer-neutral state and transitions for one command-palette surface.
#[derive(Debug)]
pub struct PaletteController {
    palette: CommandPalette,
    results: PaletteResults,
    files: PaletteFileResults,
    selection: Option<usize>,
    status: PaletteStatus,
    next_attempt: Option<PaletteAttemptId>,
    next_query_revision: Option<PaletteQueryRevision>,
}

impl PaletteController {
    #[must_use]
    pub fn new(palette: CommandPalette) -> Self {
        let results = palette.query(PaletteQuery::Browse);
        let selection = action_matches(&results)
            .filter(|matches| !matches.is_empty())
            .map(|_| 0);
        Self {
            palette,
            results,
            files: PaletteFileResults::Hidden,
            selection,
            status: PaletteStatus::Idle,
            next_attempt: Some(PaletteAttemptId::FIRST),
            next_query_revision: Some(PaletteQueryRevision::FIRST),
        }
    }

    /// Replaces visible results while preserving an invocation already in flight.
    #[must_use]
    pub fn update_query(&mut self, input: &str) -> Option<PaletteFileSearch> {
        let query = PaletteQuery::parse(input);
        self.results = self.palette.query(query);
        self.selection = (self.action_count() > 0).then_some(0);
        if !matches!(self.status, PaletteStatus::Submitting { .. }) {
            self.status = PaletteStatus::Idle;
        }
        let PaletteQuery::Search(terms) = query else {
            self.files = PaletteFileResults::Hidden;
            return None;
        };
        let Some(revision) = self.next_query_revision else {
            self.files = PaletteFileResults::RevisionsExhausted;
            return None;
        };
        self.next_query_revision = revision.next();
        self.files = PaletteFileResults::Loading(revision);
        Some(PaletteFileSearch::new(revision, terms.as_str()))
    }

    #[must_use]
    pub fn selected_action(&self) -> Option<&PaletteAction> {
        let position = self.selection?;
        self.matches()?.action_at(&self.palette, position)
    }

    /// Returns the selected file only when the row cursor addresses file results.
    #[must_use]
    pub fn selected_file(&self) -> Option<&FileSearchMatch> {
        let position = self.selection?.checked_sub(self.action_count())?;
        self.file_slice().get(position)
    }

    /// Iterates the currently visible manager-action rows.
    pub fn actions(&self) -> impl Iterator<Item = &PaletteAction> {
        self.matches()
            .into_iter()
            .flat_map(|matches| matches.actions(&self.palette))
    }

    /// Iterates file rows belonging to the latest completed local query.
    pub fn files(&self) -> impl Iterator<Item = &FileSearchMatch> {
        match self.file_search_status() {
            PaletteFileSearchStatus::Ready(files) => files,
            PaletteFileSearchStatus::Hidden
            | PaletteFileSearchStatus::Loading
            | PaletteFileSearchStatus::Failed(_)
            | PaletteFileSearchStatus::RevisionsExhausted => &[],
        }
        .iter()
    }

    /// Returns the current file-provider projection without exposing worker state.
    #[must_use]
    pub fn file_search_status(&self) -> PaletteFileSearchStatus<'_> {
        match &self.files {
            PaletteFileResults::Hidden => PaletteFileSearchStatus::Hidden,
            PaletteFileResults::Loading(_) => PaletteFileSearchStatus::Loading,
            PaletteFileResults::Ready(files) => PaletteFileSearchStatus::Ready(files),
            PaletteFileResults::Failed(error) => PaletteFileSearchStatus::Failed(*error),
            PaletteFileResults::RevisionsExhausted => PaletteFileSearchStatus::RevisionsExhausted,
        }
    }

    /// Applies file results only while their query revision is still current.
    pub fn complete_file_search(
        &mut self,
        completion: PaletteFileSearchCompletion,
    ) -> PaletteFileSearchCompletionDisposition {
        if !matches!(self.files, PaletteFileResults::Loading(revision) if revision == completion.revision)
        {
            return PaletteFileSearchCompletionDisposition::IgnoredStale;
        }
        self.files = match completion.result {
            Ok(files) => {
                if self.selection.is_none() && !files.is_empty() {
                    self.selection = Some(0);
                }
                PaletteFileResults::Ready(files.into_boxed_slice())
            }
            Err(error) => PaletteFileResults::Failed(error),
        };
        PaletteFileSearchCompletionDisposition::Applied
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
        self.row_count() == 0
    }

    #[must_use]
    pub fn selected_position(&self) -> Option<usize> {
        self.selection
    }

    /// Selects one visible result, returning whether the position exists.
    pub fn select_position(&mut self, position: usize) -> bool {
        if position >= self.row_count() {
            return false;
        }
        self.selection = Some(position);
        true
    }

    pub fn move_selection(&mut self, movement: crate::PaletteSelectionMove) {
        let Some(selected) = self.selection else {
            return;
        };
        let row_count = self.row_count();
        self.selection = Some(match movement {
            crate::PaletteSelectionMove::Next if selected + 1 == row_count => 0,
            crate::PaletteSelectionMove::Next => selected + 1,
            crate::PaletteSelectionMove::Previous if selected == 0 => row_count - 1,
            crate::PaletteSelectionMove::Previous => selected - 1,
        });
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
            PaletteResults::Actions(_) => self.activate_local(),
        }
    }

    fn activate_local(&mut self) -> Option<PaletteEffect> {
        if self.selected_action().is_some() {
            self.activate_action()
        } else {
            self.activate_file()
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

    fn activate_file(&mut self) -> Option<PaletteEffect> {
        let file = self.selected_file()?;
        let id = file.id().clone();
        let attempt = self.begin_submission(file.display_path().into())?;
        Some(PaletteEffect::File(PaletteFileInvocation::new(attempt, id)))
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
        action_matches(&self.results)
    }

    fn action_count(&self) -> usize {
        self.matches().map_or(0, PaletteMatches::len)
    }

    fn file_slice(&self) -> &[FileSearchMatch] {
        match &self.files {
            PaletteFileResults::Ready(files) => files,
            PaletteFileResults::Hidden
            | PaletteFileResults::Loading(_)
            | PaletteFileResults::Failed(_)
            | PaletteFileResults::RevisionsExhausted => &[],
        }
    }

    fn row_count(&self) -> usize {
        self.action_count() + self.file_slice().len()
    }
}

fn action_matches(results: &PaletteResults) -> Option<&PaletteMatches> {
    match results {
        PaletteResults::Actions(matches) => Some(matches),
        PaletteResults::WebPrompt | PaletteResults::WebSearch(_) => None,
    }
}

#[derive(Debug)]
enum PaletteFileResults {
    Hidden,
    Loading(PaletteQueryRevision),
    Ready(Box<[FileSearchMatch]>),
    Failed(PaletteFileSearchFailure),
    RevisionsExhausted,
}

/// Renderer-neutral state of the replaceable file-result projection.
#[derive(Clone, Copy, Debug)]
pub enum PaletteFileSearchStatus<'a> {
    Hidden,
    Loading,
    Ready(&'a [FileSearchMatch]),
    Failed(PaletteFileSearchFailure),
    RevisionsExhausted,
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
