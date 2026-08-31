# LPAC process API selection

## Decision

Use the stable Win32 `STARTUPINFOEX` AppContainer launch path for the extension
worker. Create or derive one AppContainer profile SID per plugin, supply zero
ambient capability SIDs, opt out of `ALL APPLICATION PACKAGES` to create an
LPAC, deny child processes, apply immutable creation mitigations, and assign the
suspended worker to a one-process kill-on-close Job before resuming it.

The worker independently verifies its token, capability count, integrity,
mitigation policies, and Job membership before accepting any plugin bytes. A
successful launch becomes a `VerifiedLpacWorker`; the raw process is never
exposed as an equivalent state.

## Why not `Experimental_CreateProcessInSandbox`

Microsoft's newer composable sandbox API is promising, particularly its
Bound File System policies. It is not the production backend today because:

- Microsoft marks the API experimental and subject to change.
- its header is not publicly available;
- its `SandboxSpec.fbs` input schema is not published as a supported SDK
  artifact;
- callers must dynamically resolve it from `processmodel.dll`.

Encoding an undocumented FlatBuffer would create the hardcoded compatibility
surface this project is explicitly avoiding. Keep the typed launcher boundary
so the backend can change when Microsoft publishes a stable contract.

## Worker environment

Passing a custom environment containing only `SystemRoot` fails AppContainer
creation with `ERROR_ENVVAR_NOT_FOUND`. Microsoft documents that an
AppContainer profile supplies rerouted `LOCALAPPDATA`, `TEMP`, and `TMP`
locations. The broker therefore builds exactly those three values from
`GetAppContainerFolderPath` plus `SystemRoot` from
`GetSystemWindowsDirectoryW`. It does not use a copied parent environment or
inherit process-specific variables.

## Primary sources

- [Launch an AppContainer](https://learn.microsoft.com/en-us/windows/win32/secauthz/implementing-an-appcontainer)
- [UpdateProcThreadAttribute](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute)
- [TOKEN_INFORMATION_CLASS](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ne-winnt-token_information_class)
- [CheckTokenMembershipEx](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-checktokenmembershipex)
- [Create Process in Sandbox](https://learn.microsoft.com/en-us/windows/win32/secauthz/createprocessinsandbox)
- [GetAppContainerFolderPath](https://learn.microsoft.com/en-us/windows/win32/api/userenv/nf-userenv-getappcontainerfolderpath)
