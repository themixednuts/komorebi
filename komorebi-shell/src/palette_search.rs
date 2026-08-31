use std::num::NonZeroU128;

use komorebi_search::ContentSearchLimit;
use komorebi_search::ContentSearchMatch;
use komorebi_search::ContentSearchRequestError;
use komorebi_search::ContentSearchTerms;
use komorebi_search::FileSearchClient;
use komorebi_search::FileSearchLimit;
use komorebi_search::FileSearchMatch;
use komorebi_search::FileSearchRequestError;
use thiserror::Error;

/// A revision that fences replaceable, read-only palette query results.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PaletteQueryRevision(NonZeroU128);

impl PaletteQueryRevision {
    pub(crate) const FIRST: Self = Self(NonZeroU128::MIN);

    pub(crate) fn next(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }
}

/// One typed request to a source backed by the exact-path file index.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PaletteSearch {
    Files {
        revision: PaletteQueryRevision,
        terms: Box<str>,
    },
    Content {
        revision: PaletteQueryRevision,
        terms: ContentSearchTerms,
    },
}

impl PaletteSearch {
    pub(crate) fn files(revision: PaletteQueryRevision, terms: &str) -> Self {
        Self::Files {
            revision,
            terms: terms.into(),
        }
    }

    pub(crate) const fn content(revision: PaletteQueryRevision, terms: ContentSearchTerms) -> Self {
        Self::Content { revision, terms }
    }

    #[must_use]
    pub const fn revision(&self) -> PaletteQueryRevision {
        match self {
            Self::Files { revision, .. } | Self::Content { revision, .. } => *revision,
        }
    }

    /// Completes this query through the configured owned file-index service.
    pub async fn submit(self, broker: &PaletteSearchBroker) -> PaletteSearchCompletion {
        let revision = self.revision();
        let result = match (&broker.state, self) {
            (
                PaletteSearchBrokerState::Configured {
                    client, file_limit, ..
                },
                Self::Files { terms, .. },
            ) => client
                .search(terms.into_string(), *file_limit)
                .await
                .map(PaletteSearchResults::Files)
                .map_err(PaletteSearchFailure::File),
            (
                PaletteSearchBrokerState::Configured {
                    client,
                    content_limit,
                    ..
                },
                Self::Content { terms, .. },
            ) => client
                .search_content(terms, *content_limit)
                .await
                .map(PaletteSearchResults::Content)
                .map_err(PaletteSearchFailure::Content),
            (PaletteSearchBrokerState::Unconfigured, _) => Err(PaletteSearchFailure::Unavailable),
        };
        PaletteSearchCompletion { revision, result }
    }
}

/// Runtime access to palette sources backed by one owned exact-path index.
#[derive(Clone)]
pub struct PaletteSearchBroker {
    state: PaletteSearchBrokerState,
}

impl PaletteSearchBroker {
    #[must_use]
    pub const fn configured(
        client: FileSearchClient,
        file_limit: FileSearchLimit,
        content_limit: ContentSearchLimit,
    ) -> Self {
        Self {
            state: PaletteSearchBrokerState::Configured {
                client,
                file_limit,
                content_limit,
            },
        }
    }

    #[must_use]
    pub const fn unconfigured() -> Self {
        Self {
            state: PaletteSearchBrokerState::Unconfigured,
        }
    }
}

#[derive(Clone)]
enum PaletteSearchBrokerState {
    Configured {
        client: FileSearchClient,
        file_limit: FileSearchLimit,
        content_limit: ContentSearchLimit,
    },
    Unconfigured,
}

/// One revision-bound result from an exact-path index provider.
pub struct PaletteSearchCompletion {
    pub(crate) revision: PaletteQueryRevision,
    pub(crate) result: Result<PaletteSearchResults, PaletteSearchFailure>,
}

pub(crate) enum PaletteSearchResults {
    Files(Vec<FileSearchMatch>),
    Content(Vec<ContentSearchMatch>),
}

/// Terminal failure of one replaceable index query.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum PaletteSearchFailure {
    #[error("indexed search is unavailable")]
    Unavailable,
    #[error("file-name search failed: {0}")]
    File(FileSearchRequestError),
    #[error("file-content search failed: {0}")]
    Content(ContentSearchRequestError),
}

/// Whether provider results changed the controller's current query projection.
#[must_use]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaletteSearchCompletionDisposition {
    Applied,
    IgnoredStale,
}
