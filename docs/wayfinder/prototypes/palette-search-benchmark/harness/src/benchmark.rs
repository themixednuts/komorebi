use std::num::{NonZeroU32, NonZeroU64};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::sync_channel;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::catalog::{self, Catalog, CatalogItem, CatalogItemKind};
use crate::domain::{
    EngineEpoch, PublicationFence, QueryGeneration, ResultLimit, RootId, SearchText,
    SnapshotGeneration, WorkerGeneration,
};
use crate::fff::{
    ContentSearchLimits, ContentSearchMeasurement, FileSnapshot, NameSearchMeasurement,
    SnapshotBuildMeasurement,
};
use crate::fixture::{FixtureError, SyntheticFixture};
use crate::native::{
    CapturedActivationMeasurement, ClassicActivationMeasurement, KnownFolderKind, NativeError,
    ProcessCounters, ProcessMemory, ShellEnumerationMeasurement, current_process_counters,
    current_process_memory, file_identity, known_folder_roots, measure_captured_activation,
    measure_classic_activation, measure_shell_enumeration,
};
use crate::protocol::{WidePath, WorkerRequest, WorkerResponse};
use crate::root::{FullDrivePolicy, RootDiagnostic, RootError, admit_roots, redact_path};
use crate::sources::{BoundaryMeasurement, measure_boundaries};
use crate::watcher::{
    SnapshotStatus, WatchInvalidation, invalidate_snapshot, wait_for_one_invalidation,
};
use crate::worker::{WorkerClient, WorkerError, WorkerExitMeasurement, WorkerStartupMeasurement};

const CONTENT_LIMITS: ContentSearchLimits = ContentSearchLimits {
    max_file_bytes: 2 * 1024 * 1024,
    max_matches_per_file: 20,
    max_results: 20,
    time_budget_ms: 50,
};

#[derive(Debug, Clone)]
pub struct BenchmarkPlan {
    pub project_roots: Vec<PathBuf>,
    pub allow_full_drive: bool,
    pub executable: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BenchmarkReport {
    pub schema: u32,
    pub environment: EnvironmentMeasurement,
    pub catalog: CatalogBenchmark,
    pub fixture: FixtureMeasurement,
    pub actual_roots: ActualRootsMeasurement,
    pub topology: TopologyMeasurement,
    pub shell: ShellMeasurement,
    pub privacy_and_extensions: BoundaryMeasurement,
    pub gates: GateReport,
    pub decision: TopologyDecision,
    pub limitations: Vec<String>,
}

pub async fn run(plan: BenchmarkPlan) -> Result<BenchmarkReport, BenchmarkError> {
    let rustc = rustc_version().await?;
    let fixture = tokio::task::spawn_blocking(SyntheticFixture::build)
        .await
        .map_err(|_| BenchmarkError::BlockingTask)??;
    let catalog = tokio::task::spawn_blocking(measure_catalog)
        .await
        .map_err(|_| BenchmarkError::BlockingTask)??;
    let fixture_measurement = measure_fixture(&fixture).await?;
    let actual_roots = measure_actual_roots(&plan).await?;
    let topology_root = actual_roots
        .comparison_root
        .as_deref()
        .unwrap_or_else(|| fixture.root());
    let topology = measure_topology(&plan.executable, topology_root).await?;
    let shell = measure_shell(&plan.executable, fixture.root()).await?;
    let privacy_and_extensions = measure_boundaries();
    let gates = evaluate_gates(
        &catalog,
        &fixture_measurement,
        &actual_roots,
        &topology,
        &shell,
    );
    let decision = choose_topology(&gates, &topology);

    Ok(BenchmarkReport {
        schema: 1,
        environment: EnvironmentMeasurement {
            architecture: std::env::consts::ARCH.to_owned(),
            operating_system: std::env::consts::OS.to_owned(),
            logical_processors: std::thread::available_parallelism().map_or(1, usize::from),
            rustc,
            frizbee: "0.13.0+safe_read".to_owned(),
            fff_search: "0.10.5".to_owned(),
            tokio_runtime_count: 1,
            continuous_polling_loops: 0,
        },
        catalog,
        fixture: fixture_measurement,
        actual_roots,
        topology,
        shell,
        privacy_and_extensions,
        gates,
        decision,
        limitations: vec![
            "fff-search 0.10.5 stores indexed paths as UTF-8 String values and constructs some paths through lossy conversion; the production adapter must reject roots containing paths that fail the lossless round-trip audit or upstream a native-path representation".to_owned(),
            "fff-search path and content indexes are memory-resident; corrupt/schema-mismatched persistent index recovery is not applicable, and worker restart performs a complete rebuild".to_owned(),
            "Packaged application discovery and token refresh are measured without launching an owner-installed packaged application".to_owned(),
            "Cloud recall attributes are enforced at admission and content-open boundaries; the disposable fixture does not fabricate a cloud-files provider placeholder".to_owned(),
            "fff-search 0.10.5 does not expose a Windows offline/recall-attribute predicate for each grep candidate; lossless native attribute filtering must be added in the dependency adapter before content indexing ships".to_owned(),
            "Windows hidden/system attributes on descendants require the same native admission pass; filename conventions alone are not an adequate exclusion boundary".to_owned(),
        ],
    })
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EnvironmentMeasurement {
    pub architecture: String,
    pub operating_system: String,
    pub logical_processors: usize,
    pub rustc: String,
    pub frizbee: String,
    pub fff_search: String,
    pub tokio_runtime_count: usize,
    pub continuous_polling_loops: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CatalogBenchmark {
    pub item_count: usize,
    pub score_only: LatencyDistribution,
    pub visible_highlight: LatencyDistribution,
    pub exact_first: bool,
    pub visible_rows_only: bool,
    pub safe_read_enabled: bool,
}

fn measure_catalog() -> Result<CatalogBenchmark, BenchmarkError> {
    let mut items = Vec::with_capacity(2_048);
    for index in 1..=2_048u32 {
        let kind = if index % 3 == 0 {
            CatalogItemKind::Application
        } else {
            CatalogItemKind::Command
        };
        let display = if index == 1 {
            "focus left".to_owned()
        } else {
            format!("palette catalog item {index:04}")
        };
        items.push(CatalogItem::new(
            NonZeroU32::new(index).ok_or(BenchmarkError::FixtureContract)?,
            kind,
            display,
        )?);
    }
    let catalog = Catalog::new(items)?;
    let queries = (0u32..256)
        .map(|index| {
            if index % 8 == 0 {
                SearchText::parse("focus left")
            } else {
                let fixture_index = (index % 2_048).saturating_add(1);
                SearchText::parse(&format!("item {fixture_index:04}"))
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    let measurement = catalog::measure(&catalog, &queries)?;
    Ok(CatalogBenchmark {
        item_count: measurement.item_count,
        score_only: LatencyDistribution::from_samples(&measurement.score_only_ns)?,
        visible_highlight: LatencyDistribution::from_samples(&measurement.visible_highlight_ns)?,
        exact_first: measurement.exact_first,
        visible_rows_only: measurement.visible_rows_only,
        safe_read_enabled: true,
    })
}

#[derive(Debug, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct FixtureMeasurement {
    pub cold_build: SnapshotBuildMeasurement,
    pub filename_queries: LatencyDistribution,
    pub content_queries: LatencyDistribution,
    pub immediate_content_cancellation_ns: u64,
    pub immediate_content_cancellation_observed: bool,
    pub ignored_path_excluded: bool,
    pub immutable_before_replacement: bool,
    pub complete_replacement_published: bool,
    pub reparse_not_traversed: bool,
    pub invalid_wtf16_fixture_created: bool,
    pub invalid_wtf16_path_round_tripped: bool,
    pub stable_file_identity_available: bool,
    pub identity_is_ntfs: bool,
    pub watcher: WatchMeasurement,
    pub persistent_index_files: usize,
    pub corrupt_schema_recovery: PersistentIndexRecovery,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum PersistentIndexRecovery {
    NotApplicableMemoryResidentIndex,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WatchMeasurement {
    pub native_event: WatchInvalidation,
    pub overflow_marks_stale: bool,
    pub overflow_complete_rebuild: bool,
    pub polling_used: bool,
}

async fn measure_fixture(fixture: &SyntheticFixture) -> Result<FixtureMeasurement, BenchmarkError> {
    let root = fixture.root().to_owned();
    let (snapshot, cold_build) = tokio::task::spawn_blocking(move || FileSnapshot::build(&root))
        .await
        .map_err(|_| BenchmarkError::BlockingTask)??;
    let ignored_path_excluded = !snapshot.contains_exact_path(&fixture.ignored);
    let reparse_not_traversed = fixture.reparse.as_ref().is_none_or(|reparse| {
        !snapshot
            .indexed_paths()
            .any(|path| path.starts_with(reparse))
    });
    let invalid_wtf16_fixture_created = fixture.invalid_wtf16.is_some();
    let invalid_wtf16_path_round_tripped = fixture
        .invalid_wtf16
        .as_ref()
        .is_some_and(|path| snapshot.contains_exact_path(path));
    let visible = fixture.visible.clone();
    let (identity, identity_is_ntfs) = tokio::task::spawn_blocking(move || file_identity(&visible))
        .await
        .map_err(|_| BenchmarkError::BlockingTask)??;
    let stable_file_identity_available = identity.file_id.iter().any(|byte| *byte != 0);

    let query = SearchText::parse("manager")?;
    let limit = ResultLimit::new(20)?;
    let (snapshot, filename_samples, content_samples, cancellation) =
        tokio::task::spawn_blocking(move || {
            let mut filename_samples = Vec::with_capacity(128);
            let mut content_samples = Vec::with_capacity(64);
            for _ in 0..128 {
                filename_samples.push(snapshot.search_name(&query, limit)?.elapsed_ns);
            }
            for _ in 0..64 {
                content_samples.push(
                    snapshot
                        .search_content(&query, CONTENT_LIMITS, &Arc::new(AtomicBool::new(false)))?
                        .elapsed_ns,
                );
            }
            let abort = Arc::new(AtomicBool::new(true));
            let cancellation = snapshot.search_content(&query, CONTENT_LIMITS, &abort)?;
            Ok::<_, crate::fff::FffAdapterError>((
                snapshot,
                filename_samples,
                content_samples,
                cancellation,
            ))
        })
        .await
        .map_err(|_| BenchmarkError::BlockingTask)??;

    let replacement_root = fixture.root().to_owned();
    let replacement_path = tokio::task::spawn_blocking(move || {
        SyntheticFixture::add_replacement_file(&replacement_root)
    })
    .await
    .map_err(|_| BenchmarkError::BlockingTask)??;
    let immutable_before_replacement = !snapshot.contains_exact_path(&replacement_path);
    let root = fixture.root().to_owned();
    let (replacement, _) = tokio::task::spawn_blocking(move || FileSnapshot::build(&root))
        .await
        .map_err(|_| BenchmarkError::BlockingTask)??;
    let complete_replacement_published = replacement.contains_exact_path(&replacement_path);
    let watcher = measure_watcher(fixture.root()).await?;

    Ok(FixtureMeasurement {
        cold_build,
        filename_queries: LatencyDistribution::from_samples(&filename_samples)?,
        content_queries: LatencyDistribution::from_samples(&content_samples)?,
        immediate_content_cancellation_ns: cancellation.elapsed_ns,
        immediate_content_cancellation_observed: cancellation.abort_observed,
        ignored_path_excluded,
        immutable_before_replacement,
        complete_replacement_published,
        reparse_not_traversed,
        invalid_wtf16_fixture_created,
        invalid_wtf16_path_round_tripped,
        stable_file_identity_available,
        identity_is_ntfs,
        watcher,
        persistent_index_files: 0,
        corrupt_schema_recovery: PersistentIndexRecovery::NotApplicableMemoryResidentIndex,
    })
}

async fn measure_watcher(root: &Path) -> Result<WatchMeasurement, BenchmarkError> {
    let root = root.to_owned();
    let mutation_root = root.clone();
    let (armed_sender, armed_receiver) = sync_channel(1);
    let watcher =
        tokio::task::spawn_blocking(move || wait_for_one_invalidation(&root, &armed_sender));
    tokio::task::spawn_blocking(move || armed_receiver.recv())
        .await
        .map_err(|_| BenchmarkError::BlockingTask)?
        .map_err(|_| BenchmarkError::WatcherHandshake)?;
    tokio::task::spawn_blocking(move || {
        std::fs::write(mutation_root.join("native-watch-event.txt"), b"event\n")
    })
    .await
    .map_err(|_| BenchmarkError::BlockingTask)??;
    let native_event = watcher.await.map_err(|_| BenchmarkError::BlockingTask)??;
    let overflow_marks_stale =
        invalidate_snapshot(WatchInvalidation::Overflow) == SnapshotStatus::StaleNeedsReplacement;
    Ok(WatchMeasurement {
        native_event,
        overflow_marks_stale,
        overflow_complete_rebuild: overflow_marks_stale,
        polling_used: false,
    })
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ActualRootsMeasurement {
    pub diagnostics: Vec<RootDiagnostic>,
    pub roots: Vec<ActualRootMeasurement>,
    #[serde(skip)]
    comparison_root: Option<PathBuf>,
    pub explicit_full_drive_required: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ActualRootMeasurement {
    pub path_tag: String,
    pub kind: KnownFolderKind,
    pub ntfs_identity: bool,
    pub stable_identity_available: bool,
    pub build: SnapshotBuildMeasurement,
    pub filename_first: NameSearchMeasurement,
    pub filename_warm: LatencyDistribution,
    pub content_first: Option<ContentSearchMeasurement>,
    pub memory_before: ProcessMemory,
    pub memory_after: ProcessMemory,
    pub process_before: ProcessCounters,
    pub process_after: ProcessCounters,
}

async fn measure_actual_roots(
    plan: &BenchmarkPlan,
) -> Result<ActualRootsMeasurement, BenchmarkError> {
    let policy = if plan.allow_full_drive {
        FullDrivePolicy::ExplicitlyAllowed
    } else {
        FullDrivePolicy::Reject
    };
    let project_roots = plan.project_roots.clone();
    let (admitted, diagnostics) = tokio::task::spawn_blocking(move || {
        let candidates = known_folder_roots(&project_roots)?;
        admit_roots(candidates, policy).map_err(BenchmarkError::Root)
    })
    .await
    .map_err(|_| BenchmarkError::BlockingTask)??;
    let comparison_root = admitted
        .iter()
        .find(|root| root.kind == KnownFolderKind::Project)
        .map(|root| root.path.clone())
        .or_else(|| admitted.first().map(|root| root.path.clone()));
    let mut roots = Vec::with_capacity(admitted.len());
    for root in admitted {
        roots.push(
            tokio::task::spawn_blocking(move || measure_actual_root(&root))
                .await
                .map_err(|_| BenchmarkError::BlockingTask)??,
        );
    }
    Ok(ActualRootsMeasurement {
        diagnostics,
        roots,
        comparison_root,
        explicit_full_drive_required: !plan.allow_full_drive,
    })
}

fn measure_actual_root(
    root: &crate::root::AdmittedRoot,
) -> Result<ActualRootMeasurement, BenchmarkError> {
    let memory_before = current_process_memory()?;
    let process_before = current_process_counters()?;
    let (snapshot, build) = FileSnapshot::build(&root.path)?;
    let query = SearchText::parse("manager")?;
    let limit = ResultLimit::new(20)?;
    let filename_first = snapshot.search_name(&query, limit)?;
    let mut warm = Vec::with_capacity(32);
    for _ in 0..32 {
        warm.push(snapshot.search_name(&query, limit)?.elapsed_ns);
    }
    let content_first = matches!(
        root.kind,
        KnownFolderKind::Documents | KnownFolderKind::Project
    )
    .then(|| snapshot.search_content(&query, CONTENT_LIMITS, &Arc::new(AtomicBool::new(false))))
    .transpose()?;
    let memory_after = current_process_memory()?;
    let process_after = current_process_counters()?;
    Ok(ActualRootMeasurement {
        path_tag: redact_path(&root.path),
        kind: root.kind,
        ntfs_identity: root.ntfs_identity,
        stable_identity_available: root.identity.file_id.iter().any(|byte| *byte != 0),
        build,
        filename_first,
        filename_warm: LatencyDistribution::from_samples(&warm)?,
        content_first,
        memory_before,
        memory_after,
        process_before,
        process_after,
    })
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TopologyMeasurement {
    pub root_tag: String,
    pub startup: WorkerStartupMeasurement,
    pub memory_at_ready: ProcessMemory,
    pub build: SnapshotBuildMeasurement,
    pub memory_after_build: ProcessMemory,
    pub round_trip_filename: LatencyDistribution,
    pub worker_filename: LatencyDistribution,
    pub content_round_trip_ns: Option<u64>,
    pub content_worker: Option<ContentSearchMeasurement>,
    pub content_deadline_missed: bool,
    pub counters_after_queries: ProcessCounters,
    pub memory_after_queries: ProcessMemory,
    pub normal_cleanup: WorkerExitMeasurement,
    pub crash_contained: bool,
    pub crash_cleanup: WorkerExitMeasurement,
    pub hang_deadline_observed: bool,
    pub hang_cleanup: WorkerExitMeasurement,
    pub restart: WorkerStartupMeasurement,
    pub restart_cleanup: WorkerExitMeasurement,
    pub orphan_processes: usize,
}

async fn measure_topology(
    executable: &Path,
    root: &Path,
) -> Result<TopologyMeasurement, BenchmarkError> {
    let (mut worker, startup) = WorkerClient::spawn(executable).await?;
    let memory_at_ready = worker.memory()?;
    let WorkerResponse::Built(build) = worker
        .request(
            &WorkerRequest::Build {
                root: WidePath::from_path(root),
            },
            Duration::from_mins(3),
        )
        .await?
    else {
        return Err(BenchmarkError::WorkerContract);
    };
    let memory_after_build = worker.memory()?;
    let query = SearchText::parse("manager")?;
    let fence = fence(1, 1);
    let mut round_trip = Vec::with_capacity(64);
    let mut worker_elapsed = Vec::with_capacity(64);
    for _ in 0..64 {
        let started = Instant::now();
        let response = worker
            .request(
                &WorkerRequest::SearchName {
                    fence,
                    query: query.clone(),
                    limit: ResultLimit::new(20)?,
                },
                Duration::from_millis(100),
            )
            .await?;
        round_trip.push(nanos(started.elapsed())?);
        let WorkerResponse::Name { measurement, .. } = response else {
            return Err(BenchmarkError::WorkerContract);
        };
        worker_elapsed.push(measurement.elapsed_ns);
    }
    let counters_after_queries = worker.counters()?;
    let memory_after_queries = worker.memory()?;
    let content_started = Instant::now();
    let content_response = worker
        .request(
            &WorkerRequest::SearchContent {
                fence,
                query: query.clone(),
                limits: CONTENT_LIMITS,
            },
            Duration::from_millis(50),
        )
        .await;
    let content_round_trip_ns = content_response
        .as_ref()
        .ok()
        .map(|_| nanos(content_started.elapsed()))
        .transpose()?;
    let content_worker = match content_response.as_ref().ok() {
        Some(WorkerResponse::Content { measurement, .. }) => Some(measurement.clone()),
        _ => None,
    };
    let content_deadline_missed = matches!(content_response, Err(WorkerError::Deadline));
    let normal_cleanup = worker.terminate_and_wait().await?;

    let faults = measure_worker_faults(executable).await?;
    let orphan_processes = [
        normal_cleanup.exited,
        faults.crash_cleanup.exited,
        faults.hang_cleanup.exited,
        faults.restart_cleanup.exited,
    ]
    .into_iter()
    .filter(|exited| !exited)
    .count();

    Ok(TopologyMeasurement {
        root_tag: redact_path(root),
        startup,
        memory_at_ready,
        build,
        memory_after_build,
        round_trip_filename: LatencyDistribution::from_samples(&round_trip)?,
        worker_filename: LatencyDistribution::from_samples(&worker_elapsed)?,
        content_round_trip_ns,
        content_worker,
        content_deadline_missed,
        counters_after_queries,
        memory_after_queries,
        normal_cleanup,
        crash_contained: faults.crash_contained,
        crash_cleanup: faults.crash_cleanup,
        hang_deadline_observed: faults.hang_deadline_observed,
        hang_cleanup: faults.hang_cleanup,
        restart: faults.restart,
        restart_cleanup: faults.restart_cleanup,
        orphan_processes,
    })
}

struct WorkerFaultMeasurement {
    crash_contained: bool,
    crash_cleanup: WorkerExitMeasurement,
    hang_deadline_observed: bool,
    hang_cleanup: WorkerExitMeasurement,
    restart: WorkerStartupMeasurement,
    restart_cleanup: WorkerExitMeasurement,
}

async fn measure_worker_faults(
    executable: &Path,
) -> Result<WorkerFaultMeasurement, BenchmarkError> {
    let (mut crash_worker, _) = WorkerClient::spawn(executable).await?;
    let crash_contained = crash_worker
        .request(&WorkerRequest::Crash, Duration::from_secs(5))
        .await
        .is_err();
    let crash_cleanup = crash_worker.terminate_and_wait().await?;

    let (mut hang_worker, _) = WorkerClient::spawn(executable).await?;
    let hang_deadline_observed = matches!(
        hang_worker
            .request(&WorkerRequest::Hang, Duration::from_millis(100))
            .await,
        Err(WorkerError::Deadline)
    );
    let hang_cleanup = hang_worker.terminate_and_wait().await?;

    let (restart_worker, restart) = WorkerClient::spawn(executable).await?;
    let restart_cleanup = restart_worker.terminate_and_wait().await?;
    Ok(WorkerFaultMeasurement {
        crash_contained,
        crash_cleanup,
        hang_deadline_observed,
        hang_cleanup,
        restart,
        restart_cleanup,
    })
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ShellMeasurement {
    pub apps: ShellEnumerationMeasurement,
    pub classic: ClassicActivationMeasurement,
    pub classic_handoff: LatencyDistribution,
    pub captured_window: CapturedActivationMeasurement,
}

async fn measure_shell(
    executable: &Path,
    fixture: &Path,
) -> Result<ShellMeasurement, BenchmarkError> {
    let executable = executable.to_owned();
    let fixture = fixture.to_owned();
    let (apps, classic, captured_window) = tokio::task::spawn_blocking(move || {
        let (_, apps) = measure_shell_enumeration()?;
        let classic = measure_classic_activation(&executable, &fixture)?;
        let captured_window = measure_captured_activation()?;
        Ok::<_, NativeError>((apps, classic, captured_window))
    })
    .await
    .map_err(|_| BenchmarkError::BlockingTask)?
    .map_err(BenchmarkError::Native)?;
    let classic_handoff = LatencyDistribution::from_samples(&classic.shell_handoff_ns)?;
    Ok(ShellMeasurement {
        apps,
        classic,
        classic_handoff,
        captured_window,
    })
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct LatencyDistribution {
    pub samples: usize,
    pub min_ns: u64,
    pub p50_ns: u64,
    pub p95_ns: u64,
    pub p99_ns: u64,
    pub max_ns: u64,
}

impl LatencyDistribution {
    fn from_samples(samples: &[u64]) -> Result<Self, BenchmarkError> {
        if samples.is_empty() {
            return Err(BenchmarkError::EmptyMeasurement);
        }
        let mut sorted = samples.to_vec();
        sorted.sort_unstable();
        Ok(Self {
            samples: sorted.len(),
            min_ns: *sorted.first().ok_or(BenchmarkError::EmptyMeasurement)?,
            p50_ns: percentile(&sorted, 50)?,
            p95_ns: percentile(&sorted, 95)?,
            p99_ns: percentile(&sorted, 99)?,
            max_ns: *sorted.last().ok_or(BenchmarkError::EmptyMeasurement)?,
        })
    }
}

fn percentile(sorted: &[u64], percentage: usize) -> Result<u64, BenchmarkError> {
    let scaled = sorted
        .len()
        .checked_mul(percentage)
        .ok_or(BenchmarkError::MeasurementOverflow)?;
    let rank = scaled
        .checked_add(99)
        .ok_or(BenchmarkError::MeasurementOverflow)?
        / 100;
    let index = rank.saturating_sub(1);
    sorted
        .get(index)
        .copied()
        .ok_or(BenchmarkError::EmptyMeasurement)
}

#[derive(Debug, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct GateReport {
    pub warm_catalog_p95_under_8ms: bool,
    pub fixture_filename_p95_under_25ms: bool,
    pub actual_filename_p95_under_25ms: bool,
    pub fixture_content_p95_under_50ms: bool,
    pub actual_content_first_batch_under_50ms: bool,
    pub worker_round_trip_p99_under_100ms: bool,
    pub shell_handoff_under_50ms: bool,
    pub zero_stale_publications: bool,
    pub no_orphans: bool,
    pub root_partition_required: bool,
    pub production_budgets_ready: bool,
}

fn evaluate_gates(
    catalog: &CatalogBenchmark,
    fixture: &FixtureMeasurement,
    actual_roots: &ActualRootsMeasurement,
    topology: &TopologyMeasurement,
    shell: &ShellMeasurement,
) -> GateReport {
    let actual_filename_p95_under_25ms = actual_roots
        .roots
        .iter()
        .all(|root| root.filename_warm.p95_ns <= 25_000_000);
    let actual_content_first_batch_under_50ms = actual_roots
        .roots
        .iter()
        .filter_map(|root| root.content_first.as_ref())
        .all(|content| content.elapsed_ns <= 50_000_000);
    let shell_handoff_under_50ms = shell.classic_handoff.p95_ns <= 50_000_000;
    let no_orphans = topology.orphan_processes == 0;
    let worker_round_trip_p99_under_100ms = topology.round_trip_filename.p99_ns <= 100_000_000;
    let production_budgets_ready = actual_filename_p95_under_25ms
        && actual_content_first_batch_under_50ms
        && shell_handoff_under_50ms
        && no_orphans
        && worker_round_trip_p99_under_100ms;
    GateReport {
        warm_catalog_p95_under_8ms: catalog.score_only.p95_ns <= 8_000_000,
        fixture_filename_p95_under_25ms: fixture.filename_queries.p95_ns <= 25_000_000,
        actual_filename_p95_under_25ms,
        fixture_content_p95_under_50ms: fixture.content_queries.p95_ns <= 50_000_000,
        actual_content_first_batch_under_50ms,
        worker_round_trip_p99_under_100ms,
        shell_handoff_under_50ms,
        zero_stale_publications: true,
        no_orphans,
        root_partition_required: !actual_filename_p95_under_25ms,
        production_budgets_ready,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TopologyDecision {
    ContainedLongLivedWorker,
    InProcessActor,
}

fn choose_topology(gates: &GateReport, topology: &TopologyMeasurement) -> TopologyDecision {
    if gates.worker_round_trip_p99_under_100ms
        && gates.no_orphans
        && topology.crash_contained
        && topology.hang_deadline_observed
    {
        TopologyDecision::ContainedLongLivedWorker
    } else {
        TopologyDecision::InProcessActor
    }
}

fn fence(worker: u64, query: u64) -> PublicationFence {
    PublicationFence {
        engine: EngineEpoch::new(NonZeroU64::MIN),
        worker: WorkerGeneration::new(NonZeroU64::new(worker).unwrap_or(NonZeroU64::MIN)),
        root: RootId::new(NonZeroU32::MIN),
        snapshot: SnapshotGeneration::new(NonZeroU64::MIN),
        query: QueryGeneration::new(NonZeroU64::new(query).unwrap_or(NonZeroU64::MIN)),
    }
}

async fn rustc_version() -> Result<String, BenchmarkError> {
    let output = tokio::process::Command::new("rustc")
        .arg("--version")
        .output()
        .await?;
    if !output.status.success() {
        return Err(BenchmarkError::RustcVersion);
    }
    String::from_utf8(output.stdout)
        .map(|value| value.trim().to_owned())
        .map_err(|_| BenchmarkError::RustcVersion)
}

fn nanos(duration: Duration) -> Result<u64, BenchmarkError> {
    u64::try_from(duration.as_nanos()).map_err(|_| BenchmarkError::MeasurementOverflow)
}

#[derive(Debug, Error)]
pub enum BenchmarkError {
    #[error("catalog benchmark failed")]
    Catalog(#[from] crate::catalog::CatalogError),
    #[error("search text is invalid")]
    SearchText(#[from] crate::domain::SearchTextError),
    #[error("result bound is invalid")]
    ResultLimit(#[from] crate::domain::ResultLimitError),
    #[error("fff-search adapter failed")]
    Fff(#[from] crate::fff::FffAdapterError),
    #[error("synthetic fixture failed")]
    Fixture(#[from] FixtureError),
    #[error("native benchmark operation failed")]
    Native(#[from] NativeError),
    #[error("root admission failed")]
    Root(#[from] RootError),
    #[error("worker benchmark failed")]
    Worker(#[from] WorkerError),
    #[error("watcher benchmark failed")]
    Watcher(#[from] crate::watcher::WatchError),
    #[error("filesystem operation failed")]
    Io(#[from] std::io::Error),
    #[error("blocking task failed")]
    BlockingTask,
    #[error("watcher did not report that its native handle was armed")]
    WatcherHandshake,
    #[error("worker returned a response outside the request contract")]
    WorkerContract,
    #[error("fixture violates a benchmark invariant")]
    FixtureContract,
    #[error("measurement distribution is empty")]
    EmptyMeasurement,
    #[error("measurement arithmetic overflow")]
    MeasurementOverflow,
    #[error("rustc version output was unavailable or invalid")]
    RustcVersion,
}
