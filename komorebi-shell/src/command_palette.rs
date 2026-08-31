use std::collections::BTreeMap;

use komorebi_protocol::ActionAvailability;
use komorebi_protocol::ActionCategory;
use komorebi_protocol::ActionId;
use komorebi_protocol::ActionParameter;
use komorebi_protocol::ActionUnavailability;
use komorebi_protocol::CatalogSnapshot;
use komorebi_protocol::CatalogStamp;
use komorebi_protocol::PermittedUse;
use neo_frizbee::Config;
use neo_frizbee::Matcher;
use neo_frizbee::radix_sort_matches;

use crate::ActionBinding;

const QUERY_BYTES_PER_TYPO: usize = 4;
const MINIMUM_TYPOS: u16 = 2;
const MAXIMUM_TYPOS: u16 = 6;

/// A parsed palette query whose variants determine which authority may handle it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaletteQuery<'query> {
    /// No query: present the default first-party result set.
    Browse,
    /// Search local first-party providers without granting activation authority.
    Search(PaletteSearchTerms<'query>),
    /// The web prefix is present, but no terms exist and no launch is valid.
    WebPrompt,
    /// Search the web through the dedicated brokered URL-launch path.
    WebSearch(WebSearchTerms<'query>),
}

impl<'query> PaletteQuery<'query> {
    #[must_use]
    pub fn parse(input: &'query str) -> Self {
        let input = input.trim();
        let Some(web_terms) = input.strip_prefix('!') else {
            return if input.is_empty() {
                Self::Browse
            } else {
                Self::Search(PaletteSearchTerms(input))
            };
        };
        let web_terms = web_terms.trim();
        if web_terms.is_empty() {
            Self::WebPrompt
        } else {
            Self::WebSearch(WebSearchTerms(web_terms))
        }
    }
}

/// Non-empty search terms for local first-party palette providers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaletteSearchTerms<'query>(&'query str);

impl<'query> PaletteSearchTerms<'query> {
    #[must_use]
    pub const fn as_str(self) -> &'query str {
        self.0
    }
}

/// Non-empty terms accepted only by the brokered web-search provider.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WebSearchTerms<'query>(&'query str);

impl<'query> WebSearchTerms<'query> {
    #[must_use]
    pub const fn as_str(self) -> &'query str {
        self.0
    }
}

/// Renderer-neutral results whose variant retains the authority needed to activate it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PaletteResults {
    Actions(PaletteMatches),
    WebPrompt,
    WebSearch(WebSearchRequest),
}

/// An owned, non-empty web query awaiting the configured URL-launch broker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebSearchRequest {
    terms: Box<str>,
}

impl WebSearchRequest {
    /// Creates an owned request from nonempty trimmed terms.
    #[must_use]
    pub fn new(terms: &str) -> Option<Self> {
        let terms = terms.trim();
        (!terms.is_empty()).then(|| Self {
            terms: terms.into(),
        })
    }

    fn from_validated(terms: WebSearchTerms<'_>) -> Self {
        Self {
            terms: terms.as_str().into(),
        }
    }

    #[must_use]
    pub fn terms(&self) -> &str {
        &self.terms
    }
}

/// An immutable renderer-independent index of interactive manager actions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandPalette {
    stamp: CatalogStamp,
    actions: Box<[PaletteAction]>,
}

impl CommandPalette {
    /// Projects the interactive portion of one authorized manager catalog.
    #[must_use]
    pub fn project(catalog: &CatalogSnapshot) -> Self {
        let actions = catalog
            .definitions()
            .iter()
            .zip(catalog.offers())
            .filter(|(definition, _)| {
                definition
                    .permitted_uses()
                    .contains(&PermittedUse::Interactive)
            })
            .map(|(definition, offer)| {
                PaletteAction::new(
                    definition.key().id().clone(),
                    definition.category(),
                    definition.title().as_str(),
                    definition.description().as_str(),
                    definition
                        .keywords()
                        .iter()
                        .map(komorebi_protocol::BoundedText::as_str),
                    definition.parameters(),
                    offer.availability(),
                )
            })
            .collect();
        Self {
            stamp: catalog.stamp(),
            actions,
        }
    }

    #[must_use]
    pub fn actions(&self) -> &[PaletteAction] {
        &self.actions
    }

    /// Routes one parsed query without erasing source-specific activation data.
    #[must_use]
    pub fn query(&self, query: PaletteQuery<'_>) -> PaletteResults {
        match query {
            PaletteQuery::Browse => PaletteResults::Actions(self.search("")),
            PaletteQuery::Search(terms) => PaletteResults::Actions(self.search(terms.as_str())),
            PaletteQuery::WebPrompt => PaletteResults::WebPrompt,
            PaletteQuery::WebSearch(terms) => {
                PaletteResults::WebSearch(WebSearchRequest::from_validated(terms))
            }
        }
    }

    /// Returns typo-tolerant results in descending FFF matcher score order.
    ///
    /// Raw user input enters through [`Self::query`]; this source-specific port
    /// exists for controllers that already hold validated local search terms.
    #[must_use]
    pub fn search(&self, query: &str) -> PaletteMatches {
        let query = query.trim();
        if query.is_empty() {
            return PaletteMatches::new(self.stamp, (0..self.actions.len()).collect());
        }

        let config = Config {
            max_typos: Some(typo_budget(query)),
            sort: false,
            ..Config::default()
        };
        let mut matches = Matcher::new(query, &config)
            .match_iter(self.actions.iter().map(PaletteAction::search_text))
            .collect::<Vec<_>>();
        radix_sort_matches(&mut matches);
        PaletteMatches::new(
            self.stamp,
            matches
                .into_iter()
                .filter_map(|matched| usize::try_from(matched.index).ok())
                .collect(),
        )
    }
}

fn typo_budget(query: &str) -> u16 {
    u16::try_from(query.len() / QUERY_BYTES_PER_TYPO)
        .unwrap_or(MAXIMUM_TYPOS)
        .clamp(MINIMUM_TYPOS, MAXIMUM_TYPOS)
}

/// An ordered palette result set with one internally bounded selection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaletteMatches {
    stamp: CatalogStamp,
    action_indices: Box<[usize]>,
    selected: Option<usize>,
}

impl PaletteMatches {
    fn new(stamp: CatalogStamp, action_indices: Vec<usize>) -> Self {
        let selected = (!action_indices.is_empty()).then_some(0);
        Self {
            stamp,
            action_indices: action_indices.into_boxed_slice(),
            selected,
        }
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.action_indices.len()
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.action_indices.is_empty()
    }

    #[must_use]
    pub const fn selected_position(&self) -> Option<usize> {
        self.selected
    }

    /// Selects one visible result, returning whether the position exists.
    pub fn select_position(&mut self, position: usize) -> bool {
        if position >= self.action_indices.len() {
            return false;
        }
        self.selected = Some(position);
        true
    }

    /// Resolves the selected action against the catalog projection that made this set.
    #[must_use]
    pub fn selected<'a>(&self, palette: &'a CommandPalette) -> Option<&'a PaletteAction> {
        (self.stamp == palette.stamp)
            .then_some(())
            .and(self.selected)
            .and_then(|position| self.action_indices.get(position))
            .and_then(|index| palette.actions.get(*index))
    }

    /// Iterates results against the catalog projection that made this set.
    pub fn actions<'a>(
        &'a self,
        palette: &'a CommandPalette,
    ) -> impl Iterator<Item = &'a PaletteAction> + 'a {
        let same_catalog = self.stamp == palette.stamp;
        self.action_indices
            .iter()
            .filter_map(move |index| same_catalog.then(|| palette.actions.get(*index)).flatten())
    }

    pub fn move_selection(&mut self, movement: PaletteSelectionMove) {
        let Some(selected) = self.selected else {
            return;
        };
        self.selected = Some(match movement {
            PaletteSelectionMove::Next if selected + 1 == self.action_indices.len() => 0,
            PaletteSelectionMove::Next => selected + 1,
            PaletteSelectionMove::Previous if selected == 0 => self.action_indices.len() - 1,
            PaletteSelectionMove::Previous => selected - 1,
        });
    }
}

/// Direction of one bounded palette-selection transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaletteSelectionMove {
    Previous,
    Next,
}

/// One authorized manager action projected for command-palette presentation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaletteAction {
    action: ActionId,
    category: ActionCategory,
    title: Box<str>,
    description: Box<str>,
    parameters: Box<[ActionParameter]>,
    availability: ActionAvailability,
    search_text: Box<str>,
}

impl PaletteAction {
    fn new<'a>(
        action: ActionId,
        category: ActionCategory,
        title: &str,
        description: &str,
        keywords: impl Iterator<Item = &'a str>,
        parameters: &[ActionParameter],
        availability: ActionAvailability,
    ) -> Self {
        let mut search_text = String::new();
        for value in [title, action.as_str(), description] {
            search_text.push_str(value);
            search_text.push('\n');
        }
        for keyword in keywords {
            search_text.push_str(keyword);
            search_text.push('\n');
        }
        Self {
            action,
            category,
            title: title.into(),
            description: description.into(),
            parameters: parameters.into(),
            availability,
            search_text: search_text.into_boxed_str(),
        }
    }

    fn search_text(&self) -> &str {
        &self.search_text
    }

    #[must_use]
    pub fn action_id(&self) -> &str {
        self.action.as_str()
    }

    #[must_use]
    pub const fn category(&self) -> ActionCategory {
        self.category
    }

    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }

    #[must_use]
    pub const fn availability(&self) -> ActionAvailability {
        self.availability
    }

    #[must_use]
    pub fn state(&self) -> PaletteActionState<'_> {
        match self.availability {
            ActionAvailability::Unavailable(reason) => PaletteActionState::Unavailable(reason),
            ActionAvailability::Available if self.parameters.is_empty() => {
                PaletteActionState::Ready(ActionBinding::new(self.action.clone(), BTreeMap::new()))
            }
            ActionAvailability::Available => PaletteActionState::RequiresInput(&self.parameters),
        }
    }
}

/// The only valid activation states for a projected palette action.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PaletteActionState<'a> {
    Ready(ActionBinding),
    RequiresInput(&'a [ActionParameter]),
    Unavailable(ActionUnavailability),
}
