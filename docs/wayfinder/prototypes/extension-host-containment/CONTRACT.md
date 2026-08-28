# Extension containment contract

## Decision

Use one headless Rust process per active extension principal. Embed one text-only LuaJIT state through `mlua`. Create the process suspended inside a unique LPAC identity, assign it to its no-breakaway Job Object, then resume it. Extension code cannot run before both boundaries exist.

The shared-host control is not a production fallback. It is a measurement baseline whose blast radius is every loaded extension.

## Primitive types

```rust
struct ExtensionPackageId(String);
struct ExtensionGeneration(u64);
struct GrantRevision(u64);
struct AppContainerSid(String);
struct PipeNonce([u8; 32]);
struct ClientProcessId(u32);

struct ExtensionPrincipal {
    package: ExtensionPackageId,
    generation: ExtensionGeneration,
    grant: GrantRevision,
}

struct AuthenticatedExtensionChannel {
    principal: ExtensionPrincipal,
    app_container: AppContainerSid,
    client: ClientProcessId,
    nonce: PipeNonce,
}
```

`AuthenticatedExtensionChannel` is only constructible after every kernel and protocol check passes. A connected pipe handle is not this type.

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
      -> constant_time_nonce_check
      -> generation_check
    -> ExtensionSupervisor::commit_active(AuthenticatedExtensionChannel)
```

The manager state thread requests activation and receives a typed outcome. It never waits on pipe I/O or process startup.

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

## Boundary ownership

- LPAC owns resource and cross-process security. `lpacAppExperience` is the only compatibility capability; adding another capability requires a new adversarial measurement.
- The Job Object owns lifetime, process-count, CPU, memory, and UI limits. It is not treated as a security identity.
- The manager owns grants, generations, restart policy, broker policy, storage, and all UI contributions.
- The protocol crate owns bounded framing and typed messages. The transport owns no domain authority.

## Packaging constraints found by the spike

- The child must not statically import User32 when Win32k is disabled. Forbidden UI APIs are probed by explicit runtime loading.
- The MSVC CRT must be statically linked or packaged beside the child; LPAC cannot depend on ambient `ALL APPLICATION PACKAGES` access.
- For an unpackaged full-trust server and LPAC client, the working pipe topology is a host-owned unqualified name. `LOCAL\` was rewritten into the AppContainer namespace and was not visible to the host-created endpoint on this machine.
- `TokenIsLessPrivilegedAppContainer` returned `ERROR_INVALID_PARAMETER` on the target build. Verification falls back to an AppContainer token whose `ALL APPLICATION PACKAGES` membership is absent.
