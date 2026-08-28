# Extension containment contract

## Decision

Use one headless Rust process per active extension principal. Embed one text-only LuaJIT state through `mlua`. Create the process suspended inside a unique LPAC identity, assign it to its no-breakaway Job Object, then resume it. Extension code cannot run before both boundaries exist.

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

`NativePath` and environment values remain `Path`/`OsStr`/`OsString` until they are encoded directly for a wide Win32 API. They never pass through `String`, `Display`, WTF-8 bytes, or a slash-normalizing interchange type. Evidence that must fit JSON carries both optional UTF-8 and the authoritative UTF-16 code units in hexadecimal.

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
    -> WindowsContainment::create_profile(principal)
      -> CreateAppContainerProfile(unique_name, lpacAppExperience)
    -> PackageStager::stage_immutable_image(profile, package)
    -> ExtensionChannelFactory::listen(principal)
      -> CreateNamedPipeW(unqualified_host_name, protected_sid_dacl, reject_remote)
    -> WindowsContainment::create_suspended_process(image, environment)
      -> CreateProcessAsUserW(explicit_application_name, no_reparsed_command_line)
      -> PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES
      -> PROC_THREAD_ATTRIBUTE_ALL_APPLICATION_PACKAGES_POLICY
      -> PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY(win32k_off)
      -> PROC_THREAD_ATTRIBUTE_CHILD_PROCESS_POLICY(restricted)
    -> WindowsContainment::assign_job(process)
      -> active_process_limit(1)
      -> kill_on_close
      -> job_memory_limit
      -> cpu_hard_cap
      -> ui_restrictions
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

Broker filesystem authorization must bind policy and action to the same opened Windows handle. Canonical path strings are suitable evidence and launch inputs, but they are not filesystem identity and must not become a check-then-open production authorization scheme.

## Boundary ownership

- LPAC owns resource and cross-process security. `lpacAppExperience` is the only compatibility capability; adding another capability requires a new adversarial measurement.
- The Job Object owns lifetime, process-count, CPU, memory, and UI limits. It is not treated as a security identity.
- The manager owns grants, generations, restart policy, broker policy, storage, and all UI contributions.
- The protocol crate owns bounded framing and typed messages. The transport owns no domain authority.

## Packaging constraints found by the spike

- The child must not statically import User32 when Win32k is disabled. Forbidden UI APIs are probed by explicit runtime loading.
- The MSVC CRT must be statically linked or packaged beside the child; LPAC cannot depend on ambient `ALL APPLICATION PACKAGES` access.
- Rust's native `OsStr::encode_wide`/`OsString::from_wide` pair is the path primitive: it round-trips potentially ill-formed UTF-16. UTF-8 path crates and the current `verbatim` crate are not used because they either change the representation or leave UNC/device forms unimplemented.
- For an unpackaged full-trust server and LPAC client, the working pipe topology is a host-owned unqualified name. `LOCAL\` was rewritten into the AppContainer namespace and was not visible to the host-created endpoint on this machine.
- `TokenIsLessPrivilegedAppContainer` returned `ERROR_INVALID_PARAMETER` on the target build. Verification falls back to an AppContainer token whose `ALL APPLICATION PACKAGES` membership is absent.
