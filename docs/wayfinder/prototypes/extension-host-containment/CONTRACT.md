# Extension containment contract

## Decision

Use one headless Rust process per active extension principal. Embed one text-only LuaJIT state through `mlua`. Classify the containment host's immediate Job before child creation. Create the extension suspended inside a unique LPAC identity, assign it to its inner Job Object, then resume it. Extension code cannot run before both boundaries exist.

Use a standalone inner Job when the host is not job-bound or when the outer Job permits explicit or silent breakaway. Use a nested inner Job, without an additional Job UI restriction, when the outer Job denies breakaway but has no UI restriction. Reject activation before child creation when the outer Job both denies breakaway and sets a UI restriction because Windows cannot form the required inner Job in that context.

The shared-host control is not a production fallback. It is a measurement baseline whose blast radius is every loaded extension.

## Primitive types

```rust
struct ExtensionPackageId(String);
struct ExtensionGeneration(NonZeroU64);
struct GrantRevision(u64);
struct AppContainerSid(String);
struct LaunchNonce([u8; 16]);
struct ClientProcessId(u32);
struct NativePath(PathBuf);
struct NativeEnvironmentBlock(Vec<u16>);
struct StoragePrincipal(ExtensionPackageId);
struct StorageKey(String);
struct StorageRevision(NonZeroU64);
struct HttpGrantRevision(NonZeroU64);
struct ApprovedHttpRequest {
    url: Url,
    host: String,
    addresses: Box<[SocketAddr]>,
}

struct ExtensionPrincipal {
    package: ExtensionPackageId,
    generation: ExtensionGeneration,
    grant: GrantRevision,
}

struct AuthenticatedExtensionChannel {
    principal: ExtensionPrincipal,
    app_container: AppContainerSid,
    client: ClientProcessId,
    nonce: LaunchNonce,
}
```

`AuthenticatedExtensionChannel` is only constructible after every kernel and protocol check passes. A connected pipe handle is not this type.

`NativePath` and environment values remain `Path`/`OsStr`/`OsString` until `widestring::U16CString` encodes them directly for a wide Win32 API. On Windows, Rust keeps `OsStr` in an opaque representation compatible with lossless WTF-8/WTF-16 round trips. Code must use `encode_wide`/`from_wide`, not inspect that representation. The Win32 boundary preserves unpaired surrogates, appends the required terminal NUL, and rejects interior NUL instead of truncating. Paths never pass through `String`, `Display`, lossy conversion, or a slash-normalizing interchange type. Evidence that must fit JSON carries both optional UTF-8 and the authoritative UTF-16 code units in hexadecimal.

Storage principals and keys are not paths. The authenticated channel supplies the principal; an extension supplies only a bounded portable logical key, expected revision, and bounded value.

## Lifecycle

```text
Installed
  -> Starting { principal }
  -> Authenticating { process, job, pipe, nonce }
  -> Active { channel }
  -> Quarantined { fault, restart_budget }
  -> Disabled { reason }
```

A generation change closes the old Job and pipe before publishing the replacement principal. Frames from an older generation are rejected; they cannot be retargeted.

The current recovery policy issues one typed `RestartPermit`. The supervisor must consume it before constructing the next nonzero generation. A second claim returns no permit, so a crash loop cannot be represented as another launch request.

## Activation call stack

```text
ExtensionRegistry::activate(package_id)
  -> ExtensionSupervisor::start(principal)
    -> LaunchJobContext::detect()
      -> IsProcessInJob(current_process)
      -> QueryInformationJobObject(immediate_job_limits)
      -> Standalone | ExplicitBreakaway | SilentBreakaway | Nested
      -> UiRestrictionsWithoutBreakaway before child creation
    -> WindowsContainment::create_profile(principal)
      -> CreateAppContainerProfile(unique_name, lpacAppExperience)
    -> PackageStager::stage_immutable_image(profile, package)
    -> ExtensionChannelFactory::listen(principal)
      -> CreateNamedPipeW(unqualified_host_name, protected_sid_dacl, reject_remote)
    -> WindowsContainment::create_suspended_process(image, environment)
      -> CreateProcessAsUserW(explicit_application_name, no_reparsed_command_line)
      -> CREATE_BREAKAWAY_FROM_JOB only for ExplicitBreakaway
      -> PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES
      -> PROC_THREAD_ATTRIBUTE_ALL_APPLICATION_PACKAGES_POLICY
      -> PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY(win32k_off)
      -> PROC_THREAD_ATTRIBUTE_CHILD_PROCESS_POLICY(restricted)
    -> WindowsContainment::assign_job(process)
      -> active_process_limit(1)
      -> kill_on_close
      -> job_memory_limit
      -> cpu_hard_cap
      -> ui_restrictions for standalone/breakaway; omitted for compatible nesting
    -> WindowsContainment::resume(process)
    -> ExtensionAuthenticator::accept(pipe, expected)
      -> GetNamedPipeClientProcessId
      -> query client token package SID and LPAC property
      -> IsProcessInJob
      -> fixed-width nonce check without early exit
      -> generation_check
    -> ExtensionSupervisor::commit_active(AuthenticatedExtensionChannel)
```

## Parent lifetime and recovery call stack

```text
LifetimeObserver::run(mode)
  -> spawn exact containment-host image with redirected kernel pipes
  -> LifetimeParent::launch(fault_child, indefinite_kernel_wait)
  -> FaultChild::arm_and_acknowledge(wait)
  -> LifetimeObserver::open_process(child_pid)
  -> LifetimeObserver::acknowledge_parent_exit()
  -> parent returns normally | parent aborts without destructors
  -> JobHandle::close_by_process_teardown()
  -> WaitForSingleObject(observed_child_handle)

ExtensionSupervisor::recover(failed_generation)
  -> OneRestartBudget::claim() -> RestartPermit
  -> ExtensionGeneration::next()
  -> ExtensionSupervisor::start(replacement_principal)
  -> ExtensionAuthenticator::accept(new_pipe, new_nonce, new_generation)
  -> GenerationGate::reject(previous_generation)
  -> OneRestartBudget::claim() -> None
```

The production manager state thread will request activation and receive a typed outcome; this disposable evidence harness runs the same launch stack synchronously.

## Nested-Job context call stack

```text
NestedJobSuite::run(context)
  -> CreateJobObjectW(outer_limits, optional_ui_restriction)
  -> spawn exact containment-host image with native Path arguments
  -> AssignProcessToJobObject(helper, outer_job)
  -> write one start signal after assignment
  -> LaunchJobContext::detect()
  -> ExtensionSupervisor::start(launch_scale)
  -> authenticated LPAC session | typed JobContextRejection
  -> WaitForSingleObject(helper_process, configured_deadline)
  -> decode bounded HelperOutcome
```

The helper blocks once before classification so assignment cannot race child creation. Completion uses the native waitable process handle. There is no status poll or settling loop.

## Native transport call stack

```text
ExtensionChannelFactory::accept(process, deadline)
  -> ConnectNamedPipe(overlapped_event)
  -> WaitForMultipleObjects(connection_event | process_exit, remaining_deadline)
  -> cancel_and_settle_exact_operation_on_deadline()

PipeChannel::receive(frame_deadline)
  -> FrameCodec::read_declared_length(max_frame_bytes)
  -> OverlappedTransfer::read_exact(manual_reset_event, total_deadline)
  -> CancelIoEx(exact_overlapped) on deadline
  -> GetOverlappedResult(wait=true) before buffer or OVERLAPPED expires
  -> FrameCodec::decode()

PipeChannel::send(frame_deadline)
  -> FrameCodec::encode(max_frame_bytes)
  -> OverlappedTransfer::write_all(manual_reset_event, total_deadline)
  -> CancelIoEx(exact_overlapped) on backpressure deadline
  -> GetOverlappedResult(wait=true) before buffer or OVERLAPPED expires
```

`PipeChannel` is a small typed state machine: `AwaitingChild -> HostMaySend -> Closed`. Progress loops exist only to finish a partially completed frame; readiness is driven by kernel events, not polling, sleeps, or one thread per channel.

## Fault-isolated manager ownership call stack

```text
ResponsivenessProbe::run(cpu_loop)
  -> ExtensionSupervisorLane::arm_fault()
    -> ArmedFault { extension, scenario, armed_at }
  -> ManagerCommandRequester::submit(sequence, reply_port)
  -> ManagerStateOwner::settle(command, current_revision)
    -> ManagerRevision::advance_checked()
    -> ManagerSettlement { sequence, revision, request_identity }
  -> requester acknowledges settlement
  -> ExtensionSupervisorLane::observe_and_terminate()
    -> ObservedFault { evidence, armed_at, observed_at }
  -> require every request and acknowledgement inside [armed_at, observed_at]
```

The extension-supervision lane owns child IPC deadlines and Job termination. The main manager-state owner never waits on an extension pipe. A zero-capacity request channel applies structural backpressure, and every spawned lane is joined on success and failure.

## Launch-distribution call stack

```text
LaunchDistribution::measure(policy.repetitions, policy.cohort_sizes)
  -> for each process count
    -> ScaleCohort::launch_named_workers()
      -> ExtensionSupervisor::start() per worker
      -> join every worker, retaining every failure
    -> ScaleCohort::measure(wall, ready, commit, echo, containment, exit)
    -> FirstObservedCohort (descriptive only; not OS-cold)
    -> WarmCohortSamples (immediate resident-cache repeats)
  -> checked percentile summaries and exact raw samples
```

Every sample receives a fresh AppContainer profile and process. The harness does not claim an OS-cold launch: Windows file and image cache state is global, and this unattended process has neither a reboot boundary nor a safe process-local cache reset. A future cold-launch claim requires a boot-orchestrated run; clearing unrelated system cache is outside this prototype's authority.

`ExtensionWorkload::LaunchScale` excludes live HTTP from launch cohorts while retaining authentication, containment probes, storage, and IPC. `ExtensionWorkload::FullBroker` is used by the detailed Rust/LuaJIT runs and restart replacement. This is a typed launch input, not an ambient flag or response cache.

Windows AF_UNIX remains a measured transport comparison, not a candidate security boundary. This probe restricts its byte `sun_path` to ASCII and rejects any endpoint that cannot be represented exactly, while the public socket surface supplies no named-pipe-equivalent client PID/token binding. The LPAC token also denied Winsock initialization on the target machine.

## Broker request call stack

```text
PipeReader::read_bounded_frame(channel)
  -> ProtocolDecoder::decode(max_64_kib)
  -> GenerationGate::validate(channel, frame)
  -> ExtensionBroker::authorize(principal, request)
    -> HttpPolicy::plan(url, method, limits)
      -> HttpAdapter::execute(plan)
    -> StoragePolicy::plan(key, revision, size)
      -> PrivateStore::compare_and_swap(plan)
  -> PipeWriter::write_bounded_outcome(channel)
```

Package code never receives a native handle, filesystem path, ambient network socket, renderer callback, or manager reference.

## Brokered HTTP call stack

```text
ExtensionBroker::fetch(authenticated_principal, input)
  -> HttpGrantGate::issue() -> HttpGrantRevision
  -> Url::parse(input)
  -> for initial request and every redirect
    -> HttpGrantGate::ensure_active(revision)
    -> HttpPolicy::authorize(url)
      -> require HTTPS, no credentials, port 443, exact configured DNS host
      -> SystemResolver::resolve(host, 443)
      -> HttpAcl::reject_non_global(addresses)
      -> ApprovedHttpRequest { normalized_url, host, exact_addresses }
    -> HttpBudget::begin_request_checked()
    -> ReqwestAdapter::execute(ApprovedHttpRequest)
      -> pin exact addresses with resolve_to_addrs
      -> TLS 1.2 minimum and hostname verification
      -> no proxy, automatic redirect, retry, referrer, decompression, or caller headers
    -> authorize redirect target through this same stack, or
    -> validate status, header bytes, MIME, Content-Length, response bytes, and total bytes
    -> check grant before and after every body read
```

The extension can submit only a URL string. It cannot submit a method, headers, proxy, resolver, socket address, retry policy, or redirect policy. DNS validation and connection use the same resolved address set, closing the check-then-resolve rebinding gap. A revoked grant fails before another hop and around every body chunk; there is no revocation poll interval.

## Durable storage call stack

```text
ExtensionBroker::put(authenticated_principal, key, expected_revision, value)
  -> StorageKey::parse(maximum_key_bytes)
  -> PrincipalStore::plan_put(current_state, limits)
    -> exact revision comparison
    -> checked value-size, entry-count, aggregate-quota, and encoded-snapshot arithmetic
    -> immutable PutPlan { next_state, next_revision }
  -> AtomicFiles::stage(create_new)
    -> write_all(serialized_snapshot)
    -> sync_all(stage_handle)
  -> AtomicFiles::promote()
    -> ReplaceFileW(active, stage, backup, WRITE_THROUGH)
    -> MoveFileExW(stage, active, WRITE_THROUGH) only for first install
  -> publish next_state only after promotion succeeds

StorageBroker::open(authenticated_principal)
  -> remove uniquely suffixed orphan stages
  -> bounded handle read (calculated maximum encoded snapshot + one sentinel byte)
  -> decode and validate schema, keys, revisions, value limits, and total quota
  -> if legacy: stage and atomically promote current schema while retaining backup

ExtensionRegistry::uninstall(principal, Retain | Delete)
  -> Retain leaves manager-private state untouched
  -> Delete removes only the validated principal subtree
```

No `exists`/metadata precheck authorizes a later storage mutation. The stage uses create-new semantics, and first-install promotion refuses to replace a racing destination. The prototype root is manager-owned and inaccessible to the LPAC principal; production should retain that ACL boundary and use handle-relative operations if a higher-privilege broker must resist a hostile same-user process. This follows the defensive filesystem rules in [Bugs Rust Won't Catch](https://corrode.dev/blog/bugs-rust-wont-catch/): do not trust a path across two system calls, preserve native path data, reject invalid boundary input, and propagate meaningful failures.

Harness-private probes follow the same creation rule. Startup recovery enumerates only the manager-owned results directory and removes direct files whose names match the complete generated `host-private-<uuid>.txt` grammar. It does not recurse or follow an arbitrary caller path. Probe creation uses `OpenOptions::create_new(true)` rather than an existence check followed by truncating creation.

Broker filesystem authorization must bind policy and action to the same opened Windows handle. Canonical path strings are suitable evidence and launch inputs, but they are not filesystem identity and must not become a check-then-open production authorization scheme.

## Boundary ownership

- LPAC owns resource and cross-process security. `lpacAppExperience` is the only compatibility capability; adding another capability requires a new adversarial measurement.
- The Job Object owns lifetime, process-count, CPU, memory, and UI limits. It is not treated as a security identity.
- The manager owns grants, generations, restart policy, broker policy, storage, and all UI contributions.
- The protocol crate owns bounded framing and typed messages. The transport owns no domain authority.

## Packaging constraints found by the spike

- The child must not statically import User32 when Win32k is disabled. Forbidden UI APIs are probed by explicit runtime loading.
- The MSVC CRT must be statically linked or packaged beside the child; LPAC cannot depend on ambient `ALL APPLICATION PACKAGES` access.
- `Path`/`OsStr`/`OsString` plus `widestring::U16CString` form the path-to-Win32 primitive: they round-trip potentially ill-formed UTF-16 and reject interior NUL. UTF-8 path crates are not used. `dunce` simplifies `\\?\` paths for legacy consumers and therefore has the wrong semantics for an authoritative boundary; verbatim, UNC, device, and normal forms remain intact. `normpath` is suitable only when a non-authoritative caller needs a normalized display or intake spelling. `wtf_string` may become useful if profiling finds repeated `OsStr` to wide re-encoding on a hot Win32 path; this prototype crosses each such boundary once, so adding it now would add a second string type without a measured benefit.
- For an unpackaged full-trust server and LPAC client, the working pipe topology is a host-owned unqualified name. `LOCAL\` was rewritten into the AppContainer namespace and was not visible to the host-created endpoint on this machine.
- `TokenIsLessPrivilegedAppContainer` returned `ERROR_INVALID_PARAMETER` on the target build. Verification falls back to an AppContainer token whose `ALL APPLICATION PACKAGES` membership is absent.
