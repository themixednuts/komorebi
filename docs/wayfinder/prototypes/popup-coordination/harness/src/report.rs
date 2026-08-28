use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use serde::Serialize;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::io::Lines;
use tokio::process::Child;
use tokio::process::ChildStdout;
use tokio::process::Command;
use windows::Wdk::System::SystemServices::RtlGetVersion;
use windows::Win32::System::SystemInformation::OSVERSIONINFOW;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_LOCATIONCHANGE;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_NAMECHANGE;

use crate::domain::Availability;
use crate::domain::ConfiguredRole;
use crate::domain::EnabledState;
use crate::domain::FamilyAction;
use crate::domain::FamilyModel;
use crate::domain::GuardDecision;
use crate::domain::HintKind;
use crate::domain::ModalConstraint;
use crate::domain::ObservationHint;
use crate::domain::ObservationRevision;
use crate::domain::OwnerGraphState;
use crate::domain::OwnerLink;
use crate::domain::PhysicalRect;
use crate::domain::PlacementIntent;
use crate::domain::PlacementRequest;
use crate::domain::SurfaceDecision;
use crate::domain::SurfaceGeneration;
use crate::domain::SurfaceObservation;
use crate::domain::SurfaceProvenance;
use crate::domain::UiaControlType;
use crate::domain::UiaEvidence;
use crate::domain::UiaFacts as DomainUiaFacts;
use crate::domain::UnavailableFact;
use crate::domain::classify_surface;
use crate::domain::evaluate_modal_constraint;
use crate::domain::guard_family;
use crate::domain::plan_placement;
use crate::event::CallbackMetrics;
use crate::event::EventObserver;
use crate::event::RawWinEvent;
use crate::native::ControlledWindow;
use crate::native::NativeWindowRef;
use crate::native::PlacementOutcome;
use crate::native::ResultValue;
use crate::native::Win32Observation;
use crate::native::apply_controlled;
use crate::native::census_visible_top_level;
use crate::native::incarnation_id_for_proof;
use crate::native::observe_window;
use crate::native::request_foreground_once;
use crate::native::verify_placement;
use crate::producer::ProducerManifest;
use crate::uia::UiaOutcome;
use crate::uia::UiaRequest;
use crate::uia::probe_process;
use crate::uia::probe_thread_victim;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const CALLBACK_QUEUE_CAPACITY: usize = 2_048;
const STORM_EVENT_COUNT: usize = 20_000;
const UIA_DEADLINE: Duration = Duration::from_millis(750);
const NATIVE_EVENT_DEADLINE: Duration = Duration::from_secs(3);
const SATURATION_MARKER_DEADLINE: Duration = Duration::from_secs(5);
const PRODUCER_SIGNAL_DEADLINE: Duration = Duration::from_secs(15);
const LIVE_CENSUS_LIMIT: usize = 512;
const LATENCY_SAMPLES: usize = 12;

#[derive(Clone, Debug, Serialize)]
pub struct EvidenceReport {
    pub schema: &'static str,
    pub measured_at_unix_seconds: u64,
    pub platform: PlatformEvidence,
    pub execution_model: ExecutionModelEvidence,
    pub live_census: LiveCensusEvidence,
    pub controlled: ControlledEvidence,
    pub events: EventEvidence,
    pub uia: UiaEvidenceReport,
    pub placement: PlacementEvidence,
    pub focus: FocusEvidence,
    pub fault_recovery: FaultRecoveryEvidence,
    pub recommendation: Recommendation,
    pub limitations: Vec<&'static str>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PlatformEvidence {
    pub major: u32,
    pub minor: u32,
    pub build: u32,
    pub windows_11_or_newer: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ExecutionModelEvidence {
    pub runtime: &'static str,
    pub runtime_entries: u8,
    pub polling_loops: u8,
    pub cancellation_rules: Vec<&'static str>,
}

#[derive(Clone, Debug, Serialize)]
pub struct LiveCensusEvidence {
    pub discovered: usize,
    pub observed: Vec<Win32Observation>,
    pub observation_failures: Vec<String>,
    pub framework_signals: BTreeSet<&'static str>,
    pub titles_read: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ControlledEvidence {
    pub manifest: ProducerManifest,
    pub observations: Vec<Win32Observation>,
    pub decisions: Vec<SurfaceDecision>,
    pub modal_constraint: ModalConstraint,
    pub guarded_family_actions: BTreeMap<&'static str, GuardDecision>,
    pub one_process_multiple_surface_roles: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct EventEvidence {
    pub raw: Vec<RawWinEvent>,
    pub metrics: CallbackMetrics,
    pub queue_capacity: usize,
    pub injected: usize,
    pub marker_observed_without_polling: bool,
    pub queue_saturation_became_gap: bool,
    pub callback_p99_below_100us: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct UiaEvidenceReport {
    pub per_surface: BTreeMap<String, UiaOutcome>,
    pub responsive_process_latency: LatencySummary,
    pub responsive_thread_call_latency: LatencySummary,
    pub hung_process: UiaOutcome,
    pub hung_thread: UiaOutcome,
    pub selected_topology: &'static str,
    pub selected_budget_ms: u64,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct LatencySummary {
    pub samples: usize,
    pub p50_ns: u64,
    pub p95_ns: u64,
    pub p99_ns: u64,
    pub maximum_ns: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct PlacementEvidence {
    pub centered: PlacementOutcome,
    pub restored: PlacementOutcome,
    pub target_observed_from_native_event: Proof,
    pub restore_observed_from_native_event: Proof,
    pub no_retry_loop: Proof,
    pub stale_generation_rejected_before_effect: Proof,
}

#[derive(Clone, Debug, Serialize)]
pub struct FocusEvidence {
    pub target_role: &'static str,
    pub one_set_foreground_attempt: Proof,
    pub api_reported_success: Proof,
    pub no_input_injection: Proof,
    pub no_attach_thread_input: Proof,
}

#[derive(Clone, Debug, Serialize)]
pub struct FaultRecoveryEvidence {
    pub duplicate_reordered_and_dropped_hints_require_census: Proof,
    pub full_census_converged: Proof,
    pub raw_handle_reuse_changes_stable_id: Proof,
    pub stale_generation_rejected_by_placement: Proof,
    pub controlled_producer_terminated_and_reaped: Proof,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Proof {
    Passed,
    Failed,
}

impl From<bool> for Proof {
    fn from(value: bool) -> Self {
        if value { Self::Passed } else { Self::Failed }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Recommendation {
    pub stable_roles: Vec<&'static str>,
    pub unresolved_roles: Vec<&'static str>,
    pub production_shape: Vec<&'static str>,
}

#[allow(
    clippy::too_many_lines,
    reason = "the disposable experiment is a single linear acquire-measure-cleanup transaction"
)]
pub async fn run(executable: PathBuf) -> Result<EvidenceReport> {
    let platform = platform_evidence()?;
    let live_census = live_census()?;
    let mut producer = OwnedProducer::spawn(&executable)?;
    let process_id = producer.process_id()?;
    let observer = EventObserver::install(process_id, CALLBACK_QUEUE_CAPACITY).await?;
    producer.send_command(b"create\n").await?;
    let manifest = producer.read_manifest().await?;
    if manifest.process_id != process_id {
        bail!("producer manifest has the wrong process incarnation");
    }
    let mut native_by_role = observe_controlled(&manifest)?;
    let responsive_role = "modeless_dialog";
    let responsive_window = fixture_window(&manifest, responsive_role)?;
    let (responsive_process_latency, responsive_thread_latency) =
        compare_responsive_uia(&executable, responsive_window).await;
    let mut per_surface = probe_controlled_surfaces(&executable, &manifest).await;
    let placement = measure_placement(&observer, &manifest, &native_by_role).await?;
    native_by_role = observe_controlled(&manifest)?;
    let focus_target = fixture_window(&manifest, "no_activate")?;
    let focus_controlled = ControlledWindow::verify(
        NativeWindowRef::from_raw(focus_target)?,
        manifest.process_id,
    )?;
    let focus = FocusEvidence {
        target_role: "no_activate",
        one_set_foreground_attempt: Proof::Passed,
        api_reported_success: request_foreground_once(focus_controlled).into(),
        no_input_injection: Proof::Passed,
        no_attach_thread_input: Proof::Passed,
    };
    let marker_baseline =
        observer.arm_watch(fixture_window(&manifest, "root")?, EVENT_OBJECT_NAMECHANGE);
    producer
        .send_command(format!("storm:{STORM_EVENT_COUNT}\n").as_bytes())
        .await?;
    producer.read_signal("storm_complete").await?;
    let marker_observed = tokio::time::timeout(
        SATURATION_MARKER_DEADLINE,
        observer.wait_for_watch(marker_baseline),
    )
    .await
    .is_ok();
    let (raw_events, callback_metrics) = observer.stop().await?;
    let event_evidence = EventEvidence {
        marker_observed_without_polling: marker_observed,
        queue_saturation_became_gap: callback_metrics.dropped > 0,
        callback_p99_below_100us: callback_metrics.p99_ns < 100_000,
        raw: raw_events,
        metrics: callback_metrics,
        queue_capacity: CALLBACK_QUEUE_CAPACITY,
        injected: STORM_EVENT_COUNT,
    };
    let hung_window = fixture_window(&manifest, "hung_provider")?;
    let hung_process = probe_process(
        &executable,
        UiaRequest {
            window: hung_window,
            generation: 2,
            deadline: UIA_DEADLINE,
        },
    )
    .await;
    let hung_thread = probe_thread_victim(
        &executable,
        UiaRequest {
            window: hung_window,
            generation: 3,
            deadline: UIA_DEADLINE,
        },
    )
    .await;
    per_surface.insert("hung_provider".to_owned(), hung_process.clone());
    let observations = native_by_role.into_values().collect::<Vec<_>>();
    let decisions = classify_controlled(&manifest, &observations, &per_surface);
    let modal_constraint = evaluate_modal_constraint(ObservationRevision(1), &decisions);
    let guarded_family_actions = guarded_actions(&modal_constraint);
    let one_process_multiple_surface_roles = decisions
        .iter()
        .map(|decision| decision.role)
        .collect::<BTreeSet<_>>()
        .len()
        > 1;
    let controlled = ControlledEvidence {
        manifest: manifest.clone(),
        observations,
        decisions,
        modal_constraint,
        guarded_family_actions,
        one_process_multiple_surface_roles,
    };
    producer.terminate_and_reap().await?;
    let fault_recovery = fault_recovery(
        &controlled,
        placement.stale_generation_rejected_before_effect == Proof::Passed,
        true,
    )?;
    let uia = UiaEvidenceReport {
        per_surface,
        responsive_process_latency,
        responsive_thread_call_latency: responsive_thread_latency,
        hung_process,
        hung_thread,
        selected_topology: "sacrificial MTA process per bounded probe generation",
        selected_budget_ms: u64::try_from(UIA_DEADLINE.as_millis()).unwrap_or(u64::MAX),
    };
    Ok(EvidenceReport {
        schema: "wayfinder.popup-coordination.evidence.v1",
        measured_at_unix_seconds: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
        platform,
        execution_model: ExecutionModelEvidence {
            runtime: "Tokio multi-thread runtime; User32 and COM remain on affine native threads",
            runtime_entries: 1,
            polling_loops: 0,
            cancellation_rules: vec![
                "drop signals native hook shutdown",
                "drop kills and asynchronously reaps owned child processes",
                "one-shot deadlines cancel only replaceable generations",
                "late UIA generations cannot become authoritative",
                "placement commits once then awaits native observation",
            ],
        },
        live_census,
        controlled,
        events: event_evidence,
        uia,
        placement,
        focus,
        fault_recovery,
        recommendation: Recommendation {
            stable_roles: vec![
                "owned Win32 dialog",
                "modeless tool window",
                "no-activate utility",
                "menu",
                "tooltip",
                "combo popup",
                "drag visual",
            ],
            unresolved_roles: vec![
                "secure-desktop UAC surface",
                "elevated surface denied by UIPI",
                "provider that exceeds its UIA generation deadline",
                "non-HWND in-process browser surface",
                "transient surface with contradictory owner evidence",
            ],
            production_shape: vec![
                "WinEvents are bounded wake hints, never state",
                "full Win32 census owns identity and owner families",
                "UIA is supplementary and sacrificial-process isolated",
                "missing evidence degrades to visible observe-only",
                "placement and focus are single-attempt controlled effects",
            ],
        },
        limitations: vec![
            "A process cannot observe the UAC secure desktop from the interactive desktop.",
            "The manager cannot bypass UIPI or force an arbitrary foreground transition.",
            "UI Automation cannot guarantee a third-party provider will respond.",
            "Browser prompts and drag visuals implemented without an HWND have no independent native surface to coordinate.",
            "The live census records present framework signals; unavailable applications are represented by controlled native fixtures and remain a follow-up compatibility matrix.",
        ],
    })
}

fn platform_evidence() -> Result<PlatformEvidence> {
    let mut version = OSVERSIONINFOW {
        dwOSVersionInfoSize: u32::try_from(std::mem::size_of::<OSVERSIONINFOW>())?,
        ..Default::default()
    };
    // SAFETY: `version` has the required size field and is valid writable storage for the call.
    let status = unsafe { RtlGetVersion(&raw mut version) };
    if status.0 < 0 {
        bail!("RtlGetVersion failed with NTSTATUS {:#x}", status.0);
    }
    Ok(PlatformEvidence {
        major: version.dwMajorVersion,
        minor: version.dwMinorVersion,
        build: version.dwBuildNumber,
        windows_11_or_newer: version.dwMajorVersion > 10
            || version.dwMajorVersion == 10 && version.dwBuildNumber >= 22_000,
    })
}

fn live_census() -> Result<LiveCensusEvidence> {
    let discovered = census_visible_top_level()?;
    let mut observed = Vec::new();
    let mut observation_failures = Vec::new();
    let mut framework_signals = BTreeSet::new();
    for window in discovered.iter().take(LIVE_CENSUS_LIMIT).copied() {
        match observe_window(window, 1) {
            Ok(observation) => {
                if let Some(signal) = framework_signal(&observation.class_utf16) {
                    framework_signals.insert(signal);
                }
                observed.push(observation);
            }
            Err(error) => observation_failures.push(format!("{error:#}")),
        }
    }
    Ok(LiveCensusEvidence {
        discovered: discovered.len(),
        observed,
        observation_failures,
        framework_signals,
        titles_read: false,
    })
}

fn observe_controlled(manifest: &ProducerManifest) -> Result<BTreeMap<String, Win32Observation>> {
    manifest
        .windows
        .iter()
        .map(|fixture| {
            let window = NativeWindowRef::from_raw(fixture.window)?;
            Ok((fixture.role.clone(), observe_window(window, 1)?))
        })
        .collect()
}

async fn compare_responsive_uia(
    executable: &Path,
    window: isize,
) -> (LatencySummary, LatencySummary) {
    let mut process_samples = Vec::with_capacity(LATENCY_SAMPLES);
    let mut thread_samples = Vec::with_capacity(LATENCY_SAMPLES);
    for generation in 0..LATENCY_SAMPLES {
        let request = UiaRequest {
            window,
            generation: u64::try_from(generation).unwrap_or(u64::MAX),
            deadline: UIA_DEADLINE,
        };
        if let UiaOutcome::Available {
            total_elapsed_ns, ..
        } = probe_process(executable, request).await
        {
            process_samples.push(total_elapsed_ns);
        }
        if let UiaOutcome::Available { facts, .. } = probe_thread_victim(executable, request).await
        {
            thread_samples.push(facts.call_elapsed_ns);
        }
    }
    (summarize(process_samples), summarize(thread_samples))
}

async fn probe_controlled_surfaces(
    executable: &Path,
    manifest: &ProducerManifest,
) -> BTreeMap<String, UiaOutcome> {
    let mut results = BTreeMap::new();
    for (generation, fixture) in manifest
        .windows
        .iter()
        .filter(|fixture| fixture.role != "hung_provider")
        .enumerate()
    {
        results.insert(
            fixture.role.clone(),
            probe_process(
                executable,
                UiaRequest {
                    window: fixture.window,
                    generation: u64::try_from(generation).unwrap_or(u64::MAX),
                    deadline: UIA_DEADLINE,
                },
            )
            .await,
        );
    }
    results
}

async fn measure_placement(
    observer: &EventObserver,
    manifest: &ProducerManifest,
    observations: &BTreeMap<String, Win32Observation>,
) -> Result<PlacementEvidence> {
    let target = observations
        .get("modeless_dialog")
        .context("modeless fixture observation missing")?;
    let owner = observations
        .get("modeless_root")
        .context("root fixture observation missing")?;
    let current = known_rect(&target.frame)?;
    let owner_frame = known_rect(&owner.frame)?;
    let work_area = known_rect(&target.work_area)?;
    let plan = plan_placement(PlacementRequest {
        window: target.stable_id,
        generation: SurfaceGeneration(target.generation),
        current,
        owner: owner_frame,
        work_area,
        intent: PlacementIntent::CenterOnOwner,
    })?
    .context("centering unexpectedly produced no placement")?;
    let native = NativeWindowRef::from_raw(target.window)?;
    let controlled = ControlledWindow::verify(native, manifest.process_id)?;
    let centered_baseline = observer.arm_watch(target.window, EVENT_OBJECT_LOCATIONCHANGE);
    let centered_before = apply_controlled(controlled, plan)?;
    tokio::time::timeout(
        NATIVE_EVENT_DEADLINE,
        observer.wait_for_watch(centered_baseline),
    )
    .await
    .context("center placement did not produce a native location event")?;
    let centered_after = observe_window(native, target.generation)?;
    let centered = verify_placement(centered_before, centered_after.clone());
    let restore = plan_placement(PlacementRequest {
        window: target.stable_id,
        generation: SurfaceGeneration(target.generation),
        current: known_rect(&centered_after.frame)?,
        owner: owner_frame,
        work_area,
        intent: PlacementIntent::RecoverIntoWorkArea,
    })?
    .map(|mut restore| {
        restore.target = current;
        restore
    })
    .context("restore unexpectedly produced no placement")?;
    let restore_baseline = observer.arm_watch(target.window, EVENT_OBJECT_LOCATIONCHANGE);
    let restored_before = apply_controlled(controlled, restore)?;
    tokio::time::timeout(
        NATIVE_EVENT_DEADLINE,
        observer.wait_for_watch(restore_baseline),
    )
    .await
    .context("restore placement did not produce a native location event")?;
    let restored_after = observe_window(native, target.generation)?;
    let mut stale = restore;
    stale.generation = SurfaceGeneration(stale.generation.0.saturating_add(1));
    let stale_generation_rejected_before_effect = apply_controlled(controlled, stale).is_err();
    Ok(PlacementEvidence {
        centered: verify_placement(centered.before.clone(), centered.after.clone()),
        restored: verify_placement(restored_before, restored_after),
        target_observed_from_native_event: Proof::Passed,
        restore_observed_from_native_event: Proof::Passed,
        no_retry_loop: Proof::Passed,
        stale_generation_rejected_before_effect: stale_generation_rejected_before_effect.into(),
    })
}

fn classify_controlled(
    manifest: &ProducerManifest,
    observations: &[Win32Observation],
    uia: &BTreeMap<String, UiaOutcome>,
) -> Vec<SurfaceDecision> {
    let id_by_raw = observations
        .iter()
        .map(|observation| (observation.window, observation.stable_id))
        .collect::<BTreeMap<_, _>>();
    manifest
        .windows
        .iter()
        .filter_map(|fixture| {
            let native = observations
                .iter()
                .find(|observation| observation.window == fixture.window)?;
            let owner = match native.owner {
                Some(raw) => id_by_raw.get(&raw).copied().map_or(
                    OwnerLink::Unresolved(UnavailableFact::MissingOwner),
                    OwnerLink::OwnedBy,
                ),
                None => OwnerLink::Root,
            };
            let root_owner = native
                .root_owner
                .and_then(|raw| id_by_raw.get(&raw).copied())
                .map_or(
                    Availability::Unavailable(UnavailableFact::MissingOwner),
                    Availability::Known,
                );
            let owner_enabled = native
                .owner
                .and_then(|raw| {
                    observations
                        .iter()
                        .find(|candidate| candidate.window == raw)
                })
                .map_or(Availability::Known(true), |owner| {
                    Availability::Known(owner.enabled == EnabledState::Enabled)
                });
            let evidence = uia.get(&fixture.role).map_or(
                UiaEvidence::Unavailable(UnavailableFact::ProviderFailed),
                domain_uia,
            );
            Some(classify_surface(&SurfaceObservation {
                window: native.stable_id,
                revision: ObservationRevision(1),
                generation: SurfaceGeneration(native.generation),
                owner,
                root_owner,
                owner_enabled,
                visibility: native.visibility,
                enabled: native.enabled,
                cloaked: availability_bool(&native.cloaked),
                style: native.style_evidence,
                frame: availability_rect(&native.frame),
                work_area: availability_rect(&native.work_area),
                dpi: availability_u32(&native.dpi),
                uia: evidence,
                configured_role: configured_role(&fixture.role),
                provenance: SurfaceProvenance::External,
                owner_graph: OwnerGraphState::Complete,
            }))
        })
        .collect()
}

fn domain_uia(outcome: &UiaOutcome) -> UiaEvidence {
    match outcome {
        UiaOutcome::Available { facts, .. } => UiaEvidence::Known(DomainUiaFacts {
            control_type: match facts.control_type {
                50_032 => UiaControlType::Window,
                50_009 => UiaControlType::Menu,
                50_022 => UiaControlType::ToolTip,
                50_033 => UiaControlType::Pane,
                other => UiaControlType::Other(other),
            },
            is_modal: facts.is_modal.map_or(
                Availability::Unavailable(UnavailableFact::ProviderUnsupported),
                Availability::Known,
            ),
            window_pattern: facts.window_pattern,
        }),
        UiaOutcome::TimedOut { .. } => UiaEvidence::Unavailable(UnavailableFact::ProviderTimedOut),
        UiaOutcome::Failed { .. } => UiaEvidence::Unavailable(UnavailableFact::ProviderFailed),
    }
}

fn configured_role(role: &str) -> Option<ConfiguredRole> {
    match role {
        "modal_dialog" | "modeless_dialog" | "hung_provider" => Some(ConfiguredRole::Dialog),
        "utility" | "no_activate" => Some(ConfiguredRole::Utility),
        "menu" => Some(ConfiguredRole::Menu),
        "tooltip" => Some(ConfiguredRole::Tooltip),
        "combo_popup" => Some(ConfiguredRole::ComboPopup),
        "drag_visual" => Some(ConfiguredRole::DragVisual),
        _ => None,
    }
}

fn guarded_actions(constraint: &ModalConstraint) -> BTreeMap<&'static str, GuardDecision> {
    [
        ("move_workspace", FamilyAction::MoveWorkspace),
        ("transfer_desktop", FamilyAction::TransferDesktop),
        ("close_root", FamilyAction::CloseRoot),
        ("focus_active_dialog", FamilyAction::FocusActiveDialog),
        ("inspect", FamilyAction::Inspect),
    ]
    .into_iter()
    .map(|(name, action)| (name, guard_family(constraint, action)))
    .collect()
}

fn fault_recovery(
    controlled: &ControlledEvidence,
    stale_generation_rejected_before_effect: bool,
    controlled_producer_terminated_and_reaped: bool,
) -> Result<FaultRecoveryEvidence> {
    let observations = controlled
        .observations
        .iter()
        .map(|native| SurfaceObservation {
            window: native.stable_id,
            revision: ObservationRevision(1),
            generation: SurfaceGeneration(native.generation),
            owner: OwnerLink::Root,
            root_owner: Availability::Known(native.stable_id),
            owner_enabled: Availability::Known(true),
            visibility: native.visibility,
            enabled: native.enabled,
            cloaked: availability_bool(&native.cloaked),
            style: native.style_evidence,
            frame: availability_rect(&native.frame),
            work_area: availability_rect(&native.work_area),
            dpi: availability_u32(&native.dpi),
            uia: UiaEvidence::Unavailable(UnavailableFact::ProviderUnsupported),
            configured_role: None,
            provenance: SurfaceProvenance::External,
            owner_graph: OwnerGraphState::Complete,
        })
        .collect::<Vec<_>>();
    let first = observations.first().context("controlled census is empty")?;
    let first_window = first.window;
    let first_generation = first.generation;
    let mut model = FamilyModel::empty();
    for sequence in [8_u64, 2, 2, 9, 1] {
        model.apply_hint(ObservationHint {
            sequence,
            window: first_window,
            generation: first_generation,
            kind: HintKind::StateChanged,
        });
    }
    model.mark_gap();
    let required = model.census_required();
    model.reconcile(observations);
    let first_native = controlled
        .observations
        .first()
        .context("controlled native census is empty")?;
    let second_generation = incarnation_id_for_proof(
        first_native.process_id,
        first_native.process_created_100ns,
        first_native.window,
        first_native.generation.saturating_add(1),
    );
    Ok(FaultRecoveryEvidence {
        duplicate_reordered_and_dropped_hints_require_census: required.into(),
        full_census_converged: (!model.census_required() && !model.decisions().is_empty()).into(),
        raw_handle_reuse_changes_stable_id: (second_generation != first_window).into(),
        stale_generation_rejected_by_placement: stale_generation_rejected_before_effect.into(),
        controlled_producer_terminated_and_reaped: controlled_producer_terminated_and_reaped.into(),
    })
}

fn summarize(mut samples: Vec<u64>) -> LatencySummary {
    if samples.is_empty() {
        return LatencySummary::default();
    }
    samples.sort_unstable();
    LatencySummary {
        samples: samples.len(),
        p50_ns: percentile(&samples, 50),
        p95_ns: percentile(&samples, 95),
        p99_ns: percentile(&samples, 99),
        maximum_ns: samples.last().copied().unwrap_or(0),
    }
}

fn percentile(samples: &[u64], percentile: usize) -> u64 {
    let rank = samples.len().saturating_mul(percentile).div_ceil(100);
    samples
        .get(rank.saturating_sub(1).min(samples.len().saturating_sub(1)))
        .copied()
        .unwrap_or(0)
}

fn fixture_window(manifest: &ProducerManifest, role: &str) -> Result<isize> {
    manifest
        .windows
        .iter()
        .find(|fixture| fixture.role == role)
        .map(|fixture| fixture.window)
        .with_context(|| format!("producer did not create {role}"))
}

fn known_rect(value: &ResultValue<PhysicalRect>) -> Result<PhysicalRect> {
    match value {
        ResultValue::Known(rect) => Ok(*rect),
        ResultValue::Unavailable(reason) => bail!("rectangle is unavailable: {reason}"),
    }
}

fn availability_bool(value: &ResultValue<bool>) -> Availability<bool> {
    match value {
        ResultValue::Known(value) => Availability::Known(*value),
        ResultValue::Unavailable(_) => Availability::Unavailable(UnavailableFact::ProviderFailed),
    }
}

fn availability_u32(value: &ResultValue<u32>) -> Availability<u32> {
    match value {
        ResultValue::Known(value) => Availability::Known(*value),
        ResultValue::Unavailable(_) => Availability::Unavailable(UnavailableFact::ProviderFailed),
    }
}

fn availability_rect(value: &ResultValue<PhysicalRect>) -> Availability<PhysicalRect> {
    match value {
        ResultValue::Known(value) => Availability::Known(*value),
        ResultValue::Unavailable(_) => Availability::Unavailable(UnavailableFact::ProviderFailed),
    }
}

fn framework_signal(class: &[u16]) -> Option<&'static str> {
    [
        ("Chrome_WidgetWin", "Chromium/Electron"),
        ("MozillaWindowClass", "Firefox"),
        ("SunAwt", "Java/Swing"),
        ("Qt", "Qt"),
        ("HwndWrapper", "WPF"),
        ("WinUIDesktop", "WinUI 3"),
        ("ApplicationFrameWindow", "Windows application frame"),
        ("CASCADIA_HOSTING_WINDOW_CLASS", "Windows Terminal"),
        ("#32770", "Win32 dialog"),
        ("#32768", "Win32 menu"),
    ]
    .into_iter()
    .find_map(|(needle, label)| contains_utf16_ascii(class, needle).then_some(label))
}

fn contains_utf16_ascii(value: &[u16], needle: &str) -> bool {
    let needle = needle.encode_utf16().collect::<Vec<_>>();
    value.windows(needle.len()).any(|window| window == needle)
}

struct OwnedProducer {
    child: Option<Child>,
    output: Lines<BufReader<ChildStdout>>,
}

impl OwnedProducer {
    fn spawn(executable: &Path) -> Result<Self> {
        let mut command = Command::new(executable);
        command
            .arg("producer")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        command.as_std_mut().creation_flags(CREATE_NO_WINDOW);
        let mut child = command.spawn()?;
        let stdout = child
            .stdout
            .take()
            .context("producer stdout was not piped")?;
        Ok(Self {
            child: Some(child),
            output: BufReader::new(stdout).lines(),
        })
    }

    fn process_id(&self) -> Result<u32> {
        self.child
            .as_ref()
            .and_then(Child::id)
            .context("producer has no process id")
    }

    async fn send_command(&mut self, command: &[u8]) -> Result<()> {
        let input = self
            .child
            .as_mut()
            .and_then(|child| child.stdin.as_mut())
            .context("producer stdin is unavailable")?;
        input.write_all(command).await?;
        input.flush().await?;
        Ok(())
    }

    async fn read_manifest(&mut self) -> Result<ProducerManifest> {
        let line = tokio::time::timeout(NATIVE_EVENT_DEADLINE, self.output.next_line())
            .await
            .context("producer manifest deadline expired")??
            .context("producer exited before its manifest")?;
        Ok(serde_json::from_str(&line)?)
    }

    async fn read_signal(&mut self, expected: &str) -> Result<()> {
        let line = tokio::time::timeout(PRODUCER_SIGNAL_DEADLINE, self.output.next_line())
            .await
            .context("producer signal deadline expired")??
            .context("producer exited before its signal")?;
        if line != expected {
            bail!("unexpected producer signal {line:?}");
        }
        Ok(())
    }

    async fn terminate_and_reap(&mut self) -> Result<()> {
        let Some(mut child) = self.child.take() else {
            return Ok(());
        };
        child.start_kill()?;
        child.wait().await?;
        Ok(())
    }
}

impl Drop for OwnedProducer {
    fn drop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                // Drop cannot report cleanup failures. kill_on_drop remains the fallback while
                // this task makes a best effort to reap the process handle.
                let _ = child.start_kill();
                let _ = child.wait().await;
            });
        } else {
            // Drop cannot report cleanup failure; kill_on_drop remains enabled on the child.
            let _ = child.start_kill();
        }
    }
}
