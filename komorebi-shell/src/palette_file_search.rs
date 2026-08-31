use std::num::NonZeroU128;

use komorebi_search::FileSearchClient;
use komorebi_search::FileSearchLimit;
use komorebi_search::FileSearchMatch;
use komorebi_search::FileSearchRequestError;

/// A revision that fences replaceable, read-only palette query results.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PaletteQueryRevision(NonZeroU128);

impl PaletteQueryRevision {
    pub(crate) const FIRST: Self = Self(NonZeroU128::MIN);

    pub(crate) fn next(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }
}

/// One typed request for bounded file results from the current local query.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaletteFileSearch {
    revision: PaletteQueryRevision,
    terms: Box<str>,
}

impl PaletteFileSearch {
    pub(crate) fn new(revision: PaletteQueryRevision, terms: &str) -> Self {
        Self {
            revision,
            terms: terms.into(),
        }
    }

    #[must_use]
    pub const fn revision(&self) -> PaletteQueryRevision {
        self.revision
    }

    #[must_use]
    pub fn terms(&self) -> &str {
        &self.terms
    }

    /// Completes this query through the configured owned file-index service.
    pub async fn submit(self, broker: &PaletteFileSearchBroker) -> PaletteFileSearchCompletion {
        let result = match broker {
            PaletteFileSearchBroker::Configured { client, limit } => client
                .search(self.terms.into_string(), *limit)
                .await
                .map_err(PaletteFileSearchFailure::Search),
            PaletteFileSearchBroker::Unconfigured => Err(PaletteFileSearchFailure::Unavailable),
        };
        PaletteFileSearchCompletion {
            revision: self.revision,
            result,
        }
    }
}

/// Runtime availability of the first-party file-search provider.
#[derive(Clone)]
pub enum PaletteFileSearchBroker {
    Configured {
        client: FileSearchClient,
        limit: FileSearchLimit,
    },
    Unconfigured,
}

impl PaletteFileSearchBroker {
    #[must_use]
    pub const fn configured(client: FileSearchClient, limit: FileSearchLimit) -> Self {
        Self::Configured { client, limit }
    }
}

/// One revision-bound result from the file-search provider.
pub struct PaletteFileSearchCompletion {
    pub(crate) revision: PaletteQueryRevision,
    pub(crate) result: Result<Vec<FileSearchMatch>, PaletteFileSearchFailure>,
}

/// Terminal failure of one file-result query.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaletteFileSearchFailure {
    Unavailable,
    Search(FileSearchRequestError),
}

/// Whether file results changed the controller's current query projection.
#[must_use]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaletteFileSearchCompletionDisposition {
    Applied,
    IgnoredStale,
}
