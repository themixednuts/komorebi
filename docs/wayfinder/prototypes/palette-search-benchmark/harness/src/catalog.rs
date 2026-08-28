use std::num::NonZeroU32;
use std::time::{Duration, Instant};

use frizbee::{CaseMatching, Config, Matcher, UnicodeMatching};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use unicode_normalization::UnicodeNormalization;

use crate::domain::{ResultLimit, SearchText};

const MAX_CATALOG_ITEMS: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CatalogItemId(NonZeroU32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CatalogItemKind {
    Command,
    Application,
}

#[derive(Debug, Clone)]
pub struct CatalogItem {
    pub id: CatalogItemId,
    pub kind: CatalogItemKind,
    display: String,
    normalized: String,
}

impl CatalogItem {
    pub fn new(
        id: NonZeroU32,
        kind: CatalogItemKind,
        display: String,
    ) -> Result<Self, CatalogError> {
        let normalized = display
            .nfkc()
            .flat_map(char::to_lowercase)
            .collect::<String>();
        if normalized.trim().is_empty() {
            return Err(CatalogError::EmptyItem);
        }
        Ok(Self {
            id: CatalogItemId(id),
            kind,
            display,
            normalized,
        })
    }

    #[must_use]
    pub fn display(&self) -> &str {
        &self.display
    }
}

#[derive(Debug)]
pub struct Catalog {
    items: Vec<CatalogItem>,
    normalized: Vec<String>,
    config: Config,
}

impl Catalog {
    pub fn new(items: Vec<CatalogItem>) -> Result<Self, CatalogError> {
        if items.is_empty() {
            return Err(CatalogError::Empty);
        }
        if items.len() > MAX_CATALOG_ITEMS {
            return Err(CatalogError::TooLarge {
                actual: items.len(),
                maximum: MAX_CATALOG_ITEMS,
            });
        }
        let normalized = items.iter().map(|item| item.normalized.clone()).collect();
        let config = Config::default()
            .casing(CaseMatching::Ignore)
            .unicode(UnicodeMatching::Smart);
        Ok(Self {
            items,
            normalized,
            config,
        })
    }

    pub fn search_scores(
        &self,
        query: &SearchText,
        limit: ResultLimit,
    ) -> Result<Vec<ScoredCatalogItem>, CatalogError> {
        let mut matcher = Matcher::new(query.as_str(), &self.config);
        matcher
            .match_list(&self.normalized)
            .into_iter()
            .take(limit.get())
            .map(|matched| {
                let index = usize::try_from(matched.index).map_err(|_| CatalogError::BadIndex)?;
                let item = self.items.get(index).ok_or(CatalogError::BadIndex)?;
                Ok(ScoredCatalogItem {
                    id: item.id,
                    kind: item.kind,
                    score: matched.score,
                    exact: matched.exact,
                    catalog_index: matched.index,
                })
            })
            .collect()
    }

    pub fn highlight_visible(
        &self,
        query: &SearchText,
        scored: &[ScoredCatalogItem],
        visible_rows: usize,
    ) -> Result<Vec<VisibleMatch>, CatalogError> {
        let mut matcher = Matcher::new(query.as_str(), &self.config);
        scored
            .iter()
            .take(visible_rows)
            .map(|item| {
                let index =
                    usize::try_from(item.catalog_index).map_err(|_| CatalogError::BadIndex)?;
                let catalog_item = self.items.get(index).ok_or(CatalogError::BadIndex)?;
                let haystack = self.normalized.get(index).ok_or(CatalogError::BadIndex)?;
                let match_result = matcher
                    .match_one_indices(haystack, item.catalog_index)
                    .ok_or(CatalogError::MissingVisibleMatch)?;
                Ok(VisibleMatch {
                    id: item.id,
                    display: catalog_item.display().to_owned(),
                    matched_scalar_indices: match_result.indices,
                })
            })
            .collect()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ScoredCatalogItem {
    pub id: CatalogItemId,
    pub kind: CatalogItemKind,
    pub score: u16,
    pub exact: bool,
    catalog_index: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisibleMatch {
    pub id: CatalogItemId,
    pub display: String,
    pub matched_scalar_indices: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogMeasurement {
    pub item_count: usize,
    pub samples: usize,
    pub score_only_ns: Vec<u64>,
    pub visible_highlight_ns: Vec<u64>,
    pub exact_first: bool,
    pub visible_rows_only: bool,
}

pub fn measure(
    catalog: &Catalog,
    queries: &[SearchText],
) -> Result<CatalogMeasurement, CatalogError> {
    let limit = ResultLimit::new(60).map_err(|_| CatalogError::InvalidFixture)?;
    let mut score_only_ns = Vec::with_capacity(queries.len());
    let mut visible_highlight_ns = Vec::with_capacity(queries.len());
    let mut exact_first = false;

    for query in queries {
        let started = Instant::now();
        let scored = catalog.search_scores(query, limit)?;
        score_only_ns.push(nanos(started.elapsed())?);

        let started = Instant::now();
        let visible = catalog.highlight_visible(query, &scored, 12)?;
        visible_highlight_ns.push(nanos(started.elapsed())?);
        if scored.first().is_some_and(|item| item.exact) {
            exact_first = true;
        }
        if visible.len() > 12 {
            return Err(CatalogError::InvalidFixture);
        }
    }

    Ok(CatalogMeasurement {
        item_count: catalog.items.len(),
        samples: queries.len(),
        score_only_ns,
        visible_highlight_ns,
        exact_first,
        visible_rows_only: true,
    })
}

fn nanos(duration: Duration) -> Result<u64, CatalogError> {
    u64::try_from(duration.as_nanos()).map_err(|_| CatalogError::DurationOverflow)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CatalogError {
    #[error("catalog is empty")]
    Empty,
    #[error("catalog contains an empty item")]
    EmptyItem,
    #[error("catalog has {actual} items; maximum is {maximum}")]
    TooLarge { actual: usize, maximum: usize },
    #[error("matcher returned an invalid catalog index")]
    BadIndex,
    #[error("visible match disappeared between score and highlight passes")]
    MissingVisibleMatch,
    #[error("fixture violates the catalog measurement contract")]
    InvalidFixture,
    #[error("duration does not fit the report representation")]
    DurationOverflow,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_then_highlight_preserves_identity() -> anyhow::Result<()> {
        let catalog = Catalog::new(vec![
            CatalogItem::new(
                NonZeroU32::MIN,
                CatalogItemKind::Command,
                "focus left".into(),
            )?,
            CatalogItem::new(
                NonZeroU32::new(2).ok_or(CatalogError::InvalidFixture)?,
                CatalogItemKind::Application,
                "Files".into(),
            )?,
        ])?;
        let query = SearchText::parse("focus left")?;
        let scored = catalog.search_scores(&query, ResultLimit::new(2)?)?;
        let visible = catalog.highlight_visible(&query, &scored, 1)?;
        assert_eq!(
            scored.first().map(|item| item.id),
            visible.first().map(|item| item.id)
        );
        assert!(scored.first().is_some_and(|item| item.exact));
        Ok(())
    }
}
