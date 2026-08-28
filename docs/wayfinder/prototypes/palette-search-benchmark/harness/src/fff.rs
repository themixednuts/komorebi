use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use fff_search::{
    FFFMode, FilePicker, FilePickerOptions, FuzzySearchOptions, GrepMode, GrepSearchOptions,
    PaginationArgs, QueryParser,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::domain::{ResultLimit, SearchText};

#[derive(Debug)]
pub struct FileSnapshot {
    picker: FilePicker,
}

impl FileSnapshot {
    pub fn build(root: &Path) -> Result<(Self, SnapshotBuildMeasurement), FffAdapterError> {
        let encoded_root = root
            .to_str()
            .ok_or(FffAdapterError::UnsupportedNativePath)?
            .to_owned();
        let started = Instant::now();
        let mut picker = FilePicker::new(FilePickerOptions {
            base_path: encoded_root,
            enable_mmap_cache: false,
            enable_content_indexing: false,
            mode: FFFMode::Ai,
            cache_budget: None,
            watch: false,
            follow_symlinks: false,
            enable_fs_root_scanning: false,
            enable_home_dir_scanning: false,
        })?;
        picker.collect_files()?;
        let build_ns = nanos(started.elapsed())?;
        let (path_arena_bytes, _, _) = picker.arena_bytes();
        let measurement = SnapshotBuildMeasurement {
            build_ns,
            file_count: picker.live_file_count(),
            directory_count: picker.get_dirs().len(),
            path_arena_bytes,
            persistent_index_files: 0,
            dependency_watcher_enabled: picker.has_watcher(),
        };
        Ok((Self { picker }, measurement))
    }

    pub fn search_name(
        &self,
        query: &SearchText,
        limit: ResultLimit,
    ) -> Result<NameSearchMeasurement, FffAdapterError> {
        let parsed = QueryParser::default().parse(query.as_str());
        let started = Instant::now();
        let results = self.picker.fuzzy_search(
            &parsed,
            None,
            FuzzySearchOptions {
                max_threads: 0,
                current_file: None,
                project_path: None,
                combo_boost_score_multiplier: 0,
                min_combo_count: u32::MAX,
                pagination: PaginationArgs {
                    offset: 0,
                    limit: limit.get(),
                },
            },
        );
        Ok(NameSearchMeasurement {
            elapsed_ns: nanos(started.elapsed())?,
            returned: results.items.len(),
            total_matched: results.total_matched,
            total_files: results.total_files,
        })
    }

    pub fn search_content(
        &self,
        query: &SearchText,
        limits: ContentSearchLimits,
        abort: &Arc<AtomicBool>,
    ) -> Result<ContentSearchMeasurement, FffAdapterError> {
        let parsed = QueryParser::default().parse(query.as_str());
        let started = Instant::now();
        let results = self.picker.grep(
            &parsed,
            &GrepSearchOptions {
                max_file_size: limits.max_file_bytes,
                max_matches_per_file: limits.max_matches_per_file,
                smart_case: true,
                file_offset: 0,
                page_limit: limits.max_results,
                mode: GrepMode::PlainText,
                time_budget_ms: limits.time_budget_ms,
                before_context: 0,
                after_context: 0,
                classify_definitions: false,
                trim_whitespace: false,
                abort_signal: Some(Arc::clone(abort)),
            },
        );
        let snippet_bytes = results
            .matches
            .iter()
            .try_fold(0usize, |total, item| {
                total.checked_add(item.line_content.len())
            })
            .ok_or(FffAdapterError::MeasurementOverflow)?;
        Ok(ContentSearchMeasurement {
            elapsed_ns: nanos(started.elapsed())?,
            matches: results.matches.len(),
            files_with_matches: results.files_with_matches,
            files_searched: results.total_files_searched,
            total_files: results.total_files,
            snippet_bytes,
            abort_observed: abort.load(std::sync::atomic::Ordering::Acquire),
        })
    }

    #[must_use]
    pub fn contains_exact_path(&self, path: &Path) -> bool {
        self.picker
            .get_files()
            .iter()
            .any(|file| file.absolute_path(&self.picker, self.picker.base_path()) == path)
    }

    pub fn indexed_paths(&self) -> impl Iterator<Item = PathBuf> + '_ {
        self.picker
            .get_files()
            .iter()
            .map(|file| file.absolute_path(&self.picker, self.picker.base_path()))
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ContentSearchLimits {
    pub max_file_bytes: u64,
    pub max_matches_per_file: usize,
    pub max_results: usize,
    pub time_budget_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotBuildMeasurement {
    pub build_ns: u64,
    pub file_count: usize,
    pub directory_count: usize,
    pub path_arena_bytes: usize,
    pub persistent_index_files: usize,
    pub dependency_watcher_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NameSearchMeasurement {
    pub elapsed_ns: u64,
    pub returned: usize,
    pub total_matched: usize,
    pub total_files: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentSearchMeasurement {
    pub elapsed_ns: u64,
    pub matches: usize,
    pub files_with_matches: usize,
    pub files_searched: usize,
    pub total_files: usize,
    pub snippet_bytes: usize,
    pub abort_observed: bool,
}

#[derive(Debug, Error)]
pub enum FffAdapterError {
    #[error("root cannot be represented by the dependency without loss")]
    UnsupportedNativePath,
    #[error("fff-search operation failed")]
    Dependency(#[from] fff_search::Error),
    #[error("measurement arithmetic overflow")]
    MeasurementOverflow,
    #[error("duration does not fit the report representation")]
    DurationOverflow,
}

fn nanos(duration: Duration) -> Result<u64, FffAdapterError> {
    u64::try_from(duration.as_nanos()).map_err(|_| FffAdapterError::DurationOverflow)
}
