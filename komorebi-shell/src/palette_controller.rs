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
use crate::PaletteInvocation;
use crate::PaletteMatches;
use crate::PaletteQuery;
use crate::PaletteQueryRevision;
use crate::PaletteResults;
use crate::PaletteSearch;
use crate::PaletteSearchCompletion;
use crate::PaletteSearchCompletionDisposition;
use crate::PaletteSearchFailure;
use crate::PaletteWebInvocation;
use crate::WebSearchRequest;
use komorebi_search::ContentSearchMatch;
use komorebi_search::FileSearchMatch;

/// Renderer-neutral state and transitions for one command-palette surface.
#[derive(Debug)]
pub struct PaletteController {
    palette: CommandPalette,
    results: PaletteResults,
    search_results: PaletteSearchResults,
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
            search_results: PaletteSearchResults::Hidden,
            selection,
            status: PaletteStatus::Idle,
            next_attempt: Some(PaletteAttemptId::FIRST),
            next_query_revision: Some(PaletteQueryRevision::FIRST),
        }
    }

    /// Replaces visible results while preserving an invocation already in flight.
    #[must_use]
    pub fn update_query(&mut self, input: &str) -> Option<PaletteSearch> {
        let query = PaletteQuery::parse(input);
        self.results = self.palette.query(query.clone());
        self.selection = (self.action_count() > 0).then_some(0);
        if !matches!(self.status, PaletteStatus::Submitting { .. }) {
            self.status = PaletteStatus::Idle;
        }
        let search = match query {
            PaletteQuery::Search(terms) => PaletteSearchKind::Files(terms.as_str()),
            PaletteQuery::ContentSearch(terms) => PaletteSearchKind::Content(terms),
            PaletteQuery::Browse
            | PaletteQuery::ContentPrompt
            | PaletteQuery::WebPrompt
            | PaletteQuery::WebSearch(_) => {
                self.search_results = PaletteSearchResults::Hidden;
                return None;
            }
        };
        let Some(revision) = self.next_query_revision else {
            self.search_results = PaletteSearchResults::RevisionsExhausted;
            return None;
        };
        self.next_query_revision = revision.next();
        self.search_results = PaletteSearchResults::Loading(revision);
        Some(match search {
            PaletteSearchKind::Files(terms) => PaletteSearch::files(revision, terms),
            PaletteSearchKind::Content(terms) => PaletteSearch::content(revision, terms),
        })
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

    /// Returns the selected content match when the cursor addresses content results.
    #[must_use]
    pub fn selected_content(&self) -> Option<&ContentSearchMatch> {
        let position = self.selection?.checked_sub(self.action_count())?;
        self.content_slice().get(position)
    }

    /// Iterates the currently visible manager-action rows.
    pub fn actions(&self) -> impl Iterator<Item = &PaletteAction> {
        self.matches()
            .into_iter()
            .flat_map(|matches| matches.actions(&self.palette))
    }

    /// Iterates file rows belonging to the latest completed local query.
    pub fn files(&self) -> impl Iterator<Item = &FileSearchMatch> {
        self.file_slice().iter()
    }

    /// Iterates content rows belonging to the latest explicit content query.
    pub fn content_results(&self) -> impl Iterator<Item = &ContentSearchMatch> {
        self.content_slice().iter()
    }

    /// Returns the current file-provider projection without exposing worker state.
    #[must_use]
    pub fn search_status(&self) -> PaletteSearchStatus<'_> {
        match &self.search_results {
            PaletteSearchResults::Hidden => PaletteSearchStatus::Hidden,
            PaletteSearchResults::Loading(_) => PaletteSearchStatus::Loading,
            PaletteSearchResults::Files(files) => PaletteSearchStatus::Files(files),
            PaletteSearchResults::Content(matches) => PaletteSearchStatus::Content(matches),
            PaletteSearchResults::Failed(error) => PaletteSearchStatus::Failed(*error),
            PaletteSearchResults::RevisionsExhausted => PaletteSearchStatus::RevisionsExhausted,
        }
    }

    /// Applies file results only while their query revision is still current.
    pub fn complete_search(
        &mut self,
        completion: PaletteSearchCompletion,
    ) -> PaletteSearchCompletionDisposition {
        if !matches!(self.search_results, PaletteSearchResults::Loading(revision) if revision == completion.revision)
        {
            return PaletteSearchCompletionDisposition::IgnoredStale;
        }
        self.search_results = match completion.result {
            Ok(crate::palette_search::PaletteSearchResults::Files(files)) => {
                if self.selection.is_none() && !files.is_empty() {
                    self.selection = Some(0);
                }
                PaletteSearchResults::Files(files.into_boxed_slice())
            }
            Ok(crate::palette_search::PaletteSearchResults::Content(matches)) => {
                if self.selection.is_none() && !matches.is_empty() {
                    self.selection = Some(0);
                }
                PaletteSearchResults::Content(matches.into_boxed_slice())
            }
            Err(error) => PaletteSearchResults::Failed(error),
        };
        PaletteSearchCompletionDisposition::Applied
    }

    /// Identifies the source-specific content currently presented by a renderer.
    #[must_use]
    pub fn content(&self) -> PaletteContent<'_> {
        match &self.results {
            PaletteResults::Actions(_) => PaletteContent::Actions,
            PaletteResults::ContentPrompt => PaletteContent::ContentPrompt,
            PaletteResults::ContentSearch(terms) => PaletteContent::ContentSearch(terms),
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
            PaletteResults::ContentPrompt | PaletteResults::WebPrompt => None,
            PaletteResults::Actions(_) | PaletteResults::ContentSearch(_) => self.activate_local(),
            PaletteResults::WebSearch(request) => {
                let request = request.clone();
                let attempt = self.begin_submission("Search the web".into())?;
                Some(PaletteEffect::Web(PaletteWebInvocation::new(
                    attempt, request,
                )))
            }
        }
    }

    fn activate_local(&mut self) -> Option<PaletteEffect> {
        if self.selected_action().is_some() {
            self.activate_action()
        } else if self.selected_file().is_some() {
            self.activate_file()
        } else {
            self.activate_content()
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

    fn activate_content(&mut self) -> Option<PaletteEffect> {
        let content = self.selected_content()?;
        let id = content.id().clone();
        let label = format!("{}:{}", content.display_path(), content.line_number()).into();
        let attempt = self.begin_submission(label)?;
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
        match &self.search_results {
            PaletteSearchResults::Files(files) => files,
            PaletteSearchResults::Hidden
            | PaletteSearchResults::Loading(_)
            | PaletteSearchResults::Content(_)
            | PaletteSearchResults::Failed(_)
            | PaletteSearchResults::RevisionsExhausted => &[],
        }
    }

    fn content_slice(&self) -> &[ContentSearchMatch] {
        match &self.search_results {
            PaletteSearchResults::Content(matches) => matches,
            PaletteSearchResults::Hidden
            | PaletteSearchResults::Loading(_)
            | PaletteSearchResults::Files(_)
            | PaletteSearchResults::Failed(_)
            | PaletteSearchResults::RevisionsExhausted => &[],
        }
    }

    fn row_count(&self) -> usize {
        self.action_count() + self.file_slice().len() + self.content_slice().len()
    }
}

fn action_matches(results: &PaletteResults) -> Option<&PaletteMatches> {
    match results {
        PaletteResults::Actions(matches) => Some(matches),
        PaletteResults::ContentPrompt
        | PaletteResults::ContentSearch(_)
        | PaletteResults::WebPrompt
        | PaletteResults::WebSearch(_) => None,
    }
}

enum PaletteSearchKind<'query> {
    Files(&'query str),
    Content(komorebi_search::ContentSearchTerms),
}

#[derive(Debug)]
enum PaletteSearchResults {
    Hidden,
    Loading(PaletteQueryRevision),
    Files(Box<[FileSearchMatch]>),
    Content(Box<[ContentSearchMatch]>),
    Failed(PaletteSearchFailure),
    RevisionsExhausted,
}

/// Renderer-neutral state of the replaceable exact-index projection.
#[derive(Clone, Copy, Debug)]
pub enum PaletteSearchStatus<'a> {
    Hidden,
    Loading,
    Files(&'a [FileSearchMatch]),
    Content(&'a [ContentSearchMatch]),
    Failed(PaletteSearchFailure),
    RevisionsExhausted,
}

/// Source-specific palette content without renderer or adapter types.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaletteContent<'a> {
    Actions,
    ContentPrompt,
    ContentSearch(&'a komorebi_search::ContentSearchTerms),
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
