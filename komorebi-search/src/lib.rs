//! Lossless Windows search identities and search-engine adapters.

use std::num::NonZeroU64;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use fff_search::FilePicker;
use fff_search::FilePickerOptions;
use fff_search::FuzzySearchOptions;
use fff_search::GrepMode;
use fff_search::GrepSearchOptions;
use fff_search::PaginationArgs;
use fff_search::QueryParser;
use thiserror::Error;

mod service;

pub use service::ContentSearchRequestError;
pub use service::FileSearchClient;
pub use service::FileSearchQueueCapacity;
pub use service::FileSearchRequestError;
pub use service::FileSearchService;
pub use service::FileSearchShutdownError;
pub use service::FileSearchStartError;

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

/// A validated upper bound for one page of content-search results.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContentSearchLimit(NonZeroUsize);

impl ContentSearchLimit {
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

/// Owned nonempty terms accepted by the content-search engine.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentSearchTerms(Box<str>);

impl ContentSearchTerms {
    /// Trims and owns nonempty content terms.
    pub fn new(terms: &str) -> Option<Self> {
        let terms = terms.trim();
        (!terms.is_empty()).then(|| Self(terms.into()))
    }

    /// Returns the validated terms.
    pub fn as_str(&self) -> &str {
        &self.0
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

/// One bounded content match with an opaque exact file identity.
#[derive(Clone, Debug)]
pub struct ContentSearchMatch {
    id: OpaquePathId,
    display_path: String,
    line_number: NonZeroU64,
    byte_column: usize,
    byte_offset: u64,
    line_content: Box<str>,
}

impl ContentSearchMatch {
    /// Returns the opaque identity used for activation.
    pub const fn id(&self) -> &OpaquePathId {
        &self.id
    }

    /// Returns the lossy, presentation-only relative path.
    pub fn display_path(&self) -> &str {
        &self.display_path
    }

    /// Returns the one-based source line number.
    pub const fn line_number(&self) -> NonZeroU64 {
        self.line_number
    }

    /// Returns the zero-based UTF-8 byte column reported by the matcher.
    pub const fn byte_column(&self) -> usize {
        self.byte_column
    }

    /// Returns the matched line's absolute byte offset in the file.
    pub const fn byte_offset(&self) -> u64 {
        self.byte_offset
    }

    /// Returns the bounded matched line for presentation.
    pub fn line_content(&self) -> &str {
        &self.line_content
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
                id: self.opaque_id(item),
                display_path: item.relative_path(&self.picker),
                score: score.total,
            })
            .collect()
    }

    /// Searches indexed file contents and retains exact file identities.
    ///
    /// # Errors
    ///
    /// Returns an error if the vendored engine violates its documented result
    /// indexing or one-based line-number invariants.
    pub fn search_content(
        &self,
        terms: &ContentSearchTerms,
        limit: ContentSearchLimit,
    ) -> Result<Vec<ContentSearchMatch>, ContentSearchError> {
        let parsed = QueryParser::default().parse(terms.as_str());
        let result = self.picker.grep(
            &parsed,
            &GrepSearchOptions {
                max_matches_per_file: limit.get(),
                page_limit: limit.get(),
                mode: GrepMode::Fuzzy,
                ..GrepSearchOptions::default()
            },
        );
        let file_count = result.files.len();
        result
            .matches
            .into_iter()
            .take(limit.get())
            .map(|matched| {
                let file = result.files.get(matched.file_index).ok_or(
                    ContentSearchError::InvalidFileIndex {
                        index: matched.file_index,
                        file_count,
                    },
                )?;
                let line_number = NonZeroU64::new(matched.line_number)
                    .ok_or(ContentSearchError::ZeroLineNumber)?;
                Ok(ContentSearchMatch {
                    id: self.opaque_id(file),
                    display_path: file.relative_path(&self.picker),
                    line_number,
                    byte_column: matched.col,
                    byte_offset: matched.byte_offset,
                    line_content: matched.line_content.into_boxed_str(),
                })
            })
            .collect()
    }

    /// Resolves an ID only when it originated from this exact index instance.
    pub fn resolve<'id>(&self, id: &'id OpaquePathId) -> Option<&'id Path> {
        Arc::ptr_eq(&self.identity, &id.owner).then_some(id.exact_path.as_path())
    }

    fn opaque_id(&self, file: &fff_search::FileItem) -> OpaquePathId {
        OpaquePathId {
            owner: Arc::clone(&self.identity),
            exact_path: file.absolute_path(&self.picker, self.picker.base_path()),
        }
    }
}

#[derive(Debug)]
struct IndexIdentity;

/// Failures while constructing a file index.
#[derive(Debug, Error)]
#[error("file index engine failed: {0}")]
pub struct FileIndexBuildError(#[from] fff_search::Error);

/// A violated invariant in content-search output from the vendored engine.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ContentSearchError {
    /// A match referred outside the result's deduplicated file table.
    #[error("content match file index {index} exceeded file count {file_count}")]
    InvalidFileIndex {
        /// Invalid result-local file index.
        index: usize,
        /// Number of files available to the result.
        file_count: usize,
    },
    /// The engine emitted zero for a documented one-based line number.
    #[error("content match contained a zero line number")]
    ZeroLineNumber,
}
