use std::num::{NonZeroU32, NonZeroU64, NonZeroUsize};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use unicode_normalization::UnicodeNormalization;
use uuid::Uuid;

const MAX_SEARCH_BYTES: usize = 512;
const MAX_RESULTS: usize = 60;
const MAX_EXTENSION_ROWS: usize = 20;
const MAX_EXTENSION_BYTES: usize = 64 * 1024;
const MAX_EXTENSION_SNIPPET_BYTES: usize = 512;

macro_rules! nonzero_id {
    ($name:ident, $inner:ty) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub struct $name($inner);

        impl $name {
            #[must_use]
            pub const fn new(value: $inner) -> Self {
                Self(value)
            }

            #[must_use]
            pub const fn value(self) -> $inner {
                self.0
            }
        }
    };
}

nonzero_id!(EngineEpoch, NonZeroU64);
nonzero_id!(WorkerGeneration, NonZeroU64);
nonzero_id!(RootId, NonZeroU32);
nonzero_id!(SnapshotGeneration, NonZeroU64);
nonzero_id!(QueryGeneration, NonZeroU64);
nonzero_id!(RequestId, NonZeroU64);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchText(String);

impl SearchText {
    pub fn parse(value: &str) -> Result<Self, SearchTextError> {
        let normalized = value
            .nfkc()
            .flat_map(char::to_lowercase)
            .collect::<String>();
        let normalized = normalized.trim();
        if normalized.is_empty() {
            return Err(SearchTextError::Empty);
        }
        if normalized.len() > MAX_SEARCH_BYTES {
            return Err(SearchTextError::TooLong {
                actual: normalized.len(),
                maximum: MAX_SEARCH_BYTES,
            });
        }
        Ok(Self(normalized.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SearchTextError {
    #[error("search text is empty after normalization")]
    Empty,
    #[error("search text has {actual} bytes; maximum is {maximum}")]
    TooLong { actual: usize, maximum: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResultLimit(NonZeroUsize);

impl ResultLimit {
    pub fn new(value: usize) -> Result<Self, ResultLimitError> {
        let value = NonZeroUsize::new(value).ok_or(ResultLimitError::Zero)?;
        if value.get() > MAX_RESULTS {
            return Err(ResultLimitError::TooLarge {
                actual: value.get(),
                maximum: MAX_RESULTS,
            });
        }
        Ok(Self(value))
    }

    #[must_use]
    pub const fn get(self) -> usize {
        self.0.get()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ResultLimitError {
    #[error("result limit must be nonzero")]
    Zero,
    #[error("result limit {actual} exceeds maximum {maximum}")]
    TooLarge { actual: usize, maximum: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PublicationFence {
    pub engine: EngineEpoch,
    pub worker: WorkerGeneration,
    pub root: RootId,
    pub snapshot: SnapshotGeneration,
    pub query: QueryGeneration,
}

impl PublicationFence {
    #[must_use]
    pub const fn admits(self, current: Self) -> bool {
        self.engine.value().get() == current.engine.value().get()
            && self.worker.value().get() == current.worker.value().get()
            && self.root.value().get() == current.root.value().get()
            && self.snapshot.value().get() == current.snapshot.value().get()
            && self.query.value().get() == current.query.value().get()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryRoute {
    BlendedLocal(SearchText),
    FileContent(SearchText),
    Web(WebDraft),
    Extension {
        source: ExtensionSourceName,
        query: SearchText,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebDraft {
    pub alias: Option<String>,
    pub query: SearchText,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionSourceName(String);

impl ExtensionSourceName {
    pub fn parse(value: &str) -> Result<Self, QueryParseError> {
        let normalized = value
            .nfkc()
            .flat_map(char::to_lowercase)
            .collect::<String>();
        if normalized.is_empty()
            || !normalized
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '-')
        {
            return Err(QueryParseError::InvalidExtensionSource);
        }
        Ok(Self(normalized))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub fn parse_query(value: &str, web_aliases: &[&str]) -> Result<QueryRoute, QueryParseError> {
    let trimmed = value.trim_start();
    let Some(first) = trimmed.chars().next() else {
        return Err(QueryParseError::Empty);
    };

    if first == '\\' {
        let literal = trimmed
            .get(first.len_utf8()..)
            .ok_or(QueryParseError::Empty)?;
        return SearchText::parse(literal)
            .map(QueryRoute::BlendedLocal)
            .map_err(QueryParseError::SearchText);
    }

    if first == '?' {
        let query = trimmed
            .get(first.len_utf8()..)
            .ok_or(QueryParseError::Empty)?;
        return SearchText::parse(query)
            .map(QueryRoute::FileContent)
            .map_err(QueryParseError::SearchText);
    }

    if first == '!' {
        return parse_web(trimmed, web_aliases);
    }

    if first == '@' {
        return parse_extension(trimmed);
    }

    SearchText::parse(trimmed)
        .map(QueryRoute::BlendedLocal)
        .map_err(QueryParseError::SearchText)
}

fn parse_web(value: &str, web_aliases: &[&str]) -> Result<QueryRoute, QueryParseError> {
    let without_prefix = value.get(1..).ok_or(QueryParseError::Empty)?.trim_start();
    let (first_token, remainder) = without_prefix
        .split_once(char::is_whitespace)
        .map_or((without_prefix, ""), |(token, rest)| {
            (token, rest.trim_start())
        });
    let known_alias = web_aliases
        .iter()
        .any(|alias| first_token.eq_ignore_ascii_case(alias));
    let (alias, query) = if known_alias {
        (Some(first_token.to_ascii_lowercase()), remainder)
    } else {
        (None, without_prefix)
    };
    Ok(QueryRoute::Web(WebDraft {
        alias,
        query: SearchText::parse(query).map_err(QueryParseError::SearchText)?,
    }))
}

fn parse_extension(value: &str) -> Result<QueryRoute, QueryParseError> {
    let without_prefix = value.get(1..).ok_or(QueryParseError::Empty)?;
    let (source, query) = without_prefix
        .split_once(char::is_whitespace)
        .ok_or(QueryParseError::MissingExtensionQuery)?;
    Ok(QueryRoute::Extension {
        source: ExtensionSourceName::parse(source)?,
        query: SearchText::parse(query).map_err(QueryParseError::SearchText)?,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum QueryParseError {
    #[error("query is empty")]
    Empty,
    #[error(transparent)]
    SearchText(#[from] SearchTextError),
    #[error("extension source name is invalid")]
    InvalidExtensionSource,
    #[error("extension route requires a query")]
    MissingExtensionQuery,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionLimits {
    pub rows: usize,
    pub bytes: usize,
    pub snippet_bytes: usize,
}

impl ExtensionLimits {
    pub fn new(
        rows: usize,
        bytes: usize,
        snippet_bytes: usize,
    ) -> Result<Self, ExtensionLimitError> {
        if rows == 0 || rows > MAX_EXTENSION_ROWS {
            return Err(ExtensionLimitError::Rows);
        }
        if bytes == 0 || bytes > MAX_EXTENSION_BYTES {
            return Err(ExtensionLimitError::Bytes);
        }
        if snippet_bytes > MAX_EXTENSION_SNIPPET_BYTES {
            return Err(ExtensionLimitError::Snippet);
        }
        Ok(Self {
            rows,
            bytes,
            snippet_bytes,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ExtensionLimitError {
    #[error("extension row limit is outside the accepted range")]
    Rows,
    #[error("extension byte limit is outside the accepted range")]
    Bytes,
    #[error("extension snippet limit is outside the accepted range")]
    Snippet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvocationSettlement {
    Accepted,
    Duplicate,
}

#[derive(Debug, Default)]
pub struct OneShotInvocations(std::collections::HashSet<Uuid>);

impl OneShotInvocations {
    pub fn begin(&mut self, invocation: Uuid) -> InvocationSettlement {
        if self.0.insert(invocation) {
            InvocationSettlement::Accepted
        } else {
            InvocationSettlement::Duplicate
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_parser_preserves_unknown_web_alias_as_query() -> anyhow::Result<()> {
        let QueryRoute::Web(draft) = parse_query("!nope rust", &["g", "ddg", "yt"])? else {
            anyhow::bail!("expected web route");
        };
        assert_eq!(draft.alias, None);
        assert_eq!(draft.query.as_str(), "nope rust");
        Ok(())
    }

    #[test]
    fn route_parser_recognizes_explicit_routes() -> anyhow::Result<()> {
        assert!(matches!(
            parse_query("? manager epoch", &[])?,
            QueryRoute::FileContent(_)
        ));
        assert!(matches!(
            parse_query("@docs manager epoch", &[])?,
            QueryRoute::Extension { .. }
        ));
        assert!(matches!(
            parse_query("\\!literal", &[])?,
            QueryRoute::BlendedLocal(_)
        ));
        Ok(())
    }

    #[test]
    fn every_fence_component_must_match() {
        let current = fence(2);
        assert!(current.admits(current));
        assert!(!fence(1).admits(current));
    }

    #[test]
    fn invocation_is_accepted_once() {
        let mut invocations = OneShotInvocations::default();
        let id = Uuid::new_v4();
        assert_eq!(invocations.begin(id), InvocationSettlement::Accepted);
        assert_eq!(invocations.begin(id), InvocationSettlement::Duplicate);
    }

    fn fence(query: u64) -> PublicationFence {
        PublicationFence {
            engine: EngineEpoch::new(NonZeroU64::MIN),
            worker: WorkerGeneration::new(NonZeroU64::MIN),
            root: RootId::new(NonZeroU32::MIN),
            snapshot: SnapshotGeneration::new(NonZeroU64::MIN),
            query: QueryGeneration::new(NonZeroU64::new(query).unwrap_or(NonZeroU64::MIN)),
        }
    }
}
