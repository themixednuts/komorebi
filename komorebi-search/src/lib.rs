//! Lossless Windows search identities and search-engine adapters.

use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use fff_search::FilePicker;
use fff_search::FilePickerOptions;
use fff_search::FuzzySearchOptions;
use fff_search::PaginationArgs;
use fff_search::QueryParser;
use thiserror::Error;

/// A validated upper bound for one page of file-search results.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileSearchLimit(NonZeroUsize);

impl FileSearchLimit {
    /// Creates a nonzero result limit.
    pub const fn new(limit: usize) -> Option<Self> {
        match NonZeroUsize::new(limit) {
            Some(limit) => Some(Self(limit)),
            None => None,
        }
    }

    const fn get(self) -> usize {
        self.0.get()
    }
}

/// An opaque file identity that can only be resolved by its originating index.
#[derive(Clone)]
pub struct OpaquePathId {
    owner: Arc<IndexIdentity>,
    exact_path: PathBuf,
}

impl std::fmt::Debug for OpaquePathId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OpaquePathId")
            .finish_non_exhaustive()
    }
}

/// One file match suitable for presentation and later exact-path resolution.
#[derive(Clone, Debug)]
pub struct FileSearchMatch {
    id: OpaquePathId,
    display_path: String,
    score: i32,
}

impl FileSearchMatch {
    /// Returns the opaque identity used for activation.
    pub const fn id(&self) -> &OpaquePathId {
        &self.id
    }

    /// Returns the lossy, presentation-only relative path.
    pub fn display_path(&self) -> &str {
        &self.display_path
    }

    /// Returns the engine's aggregate fuzzy-match score.
    pub const fn score(&self) -> i32 {
        self.score
    }
}

/// An immutable file index with exact path operands owned behind opaque IDs.
pub struct FileIndex {
    identity: Arc<IndexIdentity>,
    picker: FilePicker,
}

impl FileIndex {
    /// Builds an immutable index rooted at `root`.
    ///
    /// # Errors
    ///
    /// Returns an error when the root cannot be indexed.
    pub fn build(root: PathBuf) -> Result<Self, FileIndexBuildError> {
        let mut picker = FilePicker::new(FilePickerOptions {
            base_path: root,
            watch: false,
            ..FilePickerOptions::default()
        })?;
        picker.collect_files()?;

        Ok(Self {
            identity: Arc::new(IndexIdentity),
            picker,
        })
    }

    /// Searches file names and paths, returning presentation data plus opaque
    /// index-owned identities.
    pub fn search(&self, query: &str, limit: FileSearchLimit) -> Vec<FileSearchMatch> {
        let parsed = QueryParser::default().parse(query);
        let results = self.picker.fuzzy_search(
            &parsed,
            None,
            FuzzySearchOptions {
                pagination: PaginationArgs {
                    offset: 0,
                    limit: limit.get(),
                },
                ..FuzzySearchOptions::default()
            },
        );

        results
            .items
            .iter()
            .zip(results.scores)
            .map(|(item, score)| FileSearchMatch {
                id: OpaquePathId {
                    owner: Arc::clone(&self.identity),
                    exact_path: item.absolute_path(&self.picker, self.picker.base_path()),
                },
                display_path: item.relative_path(&self.picker),
                score: score.total,
            })
            .collect()
    }

    /// Resolves an ID only when it originated from this exact index instance.
    pub fn resolve<'id>(&self, id: &'id OpaquePathId) -> Option<&'id Path> {
        Arc::ptr_eq(&self.identity, &id.owner).then_some(id.exact_path.as_path())
    }
}

#[derive(Debug)]
struct IndexIdentity;

/// Failures while constructing a file index.
#[derive(Debug, Error)]
#[error("file index engine failed: {0}")]
pub struct FileIndexBuildError(#[from] fff_search::Error);
