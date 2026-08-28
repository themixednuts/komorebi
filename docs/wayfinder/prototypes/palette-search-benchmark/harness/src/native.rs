use std::ffi::{OsStr, OsString, c_void};
use std::mem::{size_of, zeroed};
use std::num::{NonZeroU32, NonZeroU64};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE, HWND, PROPERTYKEY, WAIT_OBJECT_0};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_OFFLINE,
    FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS, FILE_ATTRIBUTE_RECALL_ON_OPEN,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_SYSTEM, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_ID_INFO, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, FileIdInfo, GetFileInformationByHandleEx,
    GetVolumeInformationByHandleW, OPEN_EXISTING,
};
use windows::Win32::System::Com::StructuredStorage::{
    PROPVARIANT, PropVariantClear, PropVariantToStringAlloc,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoTaskMemFree, CoUninitialize, IPersistFile, STGM,
};
use windows::Win32::System::ProcessStatus::{
    GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
};
use windows::Win32::System::Threading::{
    CreateEventW, EVENT_MODIFY_STATE, GetCurrentProcess, GetProcessIoCounters, GetProcessTimes,
    IO_COUNTERS, OpenEventW, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, SetEvent,
    WaitForSingleObject,
};
use windows::Win32::UI::Shell::PropertiesSystem::{GPS_DEFAULT, IPropertyStore};
use windows::Win32::UI::Shell::{
    ApplicationActivationManager, BHID_EnumItems, FOLDERID_AppsFolder, FOLDERID_Desktop,
    FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_Music, FOLDERID_Pictures, FOLDERID_Videos,
    IApplicationActivationManager, IEnumShellItems, IShellItem, IShellItem2, IShellLinkW,
    KF_FLAG_DEFAULT, SEE_MASK_ASYNCOK, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOCLOSEPROCESS,
    SHELLEXECUTEINFOW, SHGetKnownFolderItem, SHGetKnownFolderPath, SIGDN_DESKTOPABSOLUTEPARSING,
    SIGDN_NORMALDISPLAY, ShellExecuteExW, ShellLink,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, IsWindow, SW_HIDE, SetForegroundWindow,
};
use windows::core::{GUID, Interface, PCWSTR, PWSTR};

const PKEY_APP_USER_MODEL_ID: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3),
    pid: 5,
};

#[derive(Debug)]
pub struct OwnedHandle(HANDLE);

impl OwnedHandle {
    pub fn new(handle: HANDLE) -> Result<Self, NativeError> {
        if handle.is_invalid() {
            return Err(NativeError::InvalidHandle);
        }
        Ok(Self(handle))
    }

    #[must_use]
    pub const fn raw(&self) -> HANDLE {
        self.0
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: this type owns the valid handle and closes it exactly once.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

#[derive(Debug)]
pub struct ComApartment;

impl ComApartment {
    pub fn sta() -> Result<Self, NativeError> {
        // SAFETY: called once on the dedicated current thread and paired with CoUninitialize.
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.ok()?;
        Ok(Self)
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        // SAFETY: this object exists only after a successful CoInitializeEx on this thread.
        unsafe { CoUninitialize() };
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KnownFolderKind {
    Desktop,
    Documents,
    Downloads,
    Pictures,
    Music,
    Videos,
    Project,
}

#[derive(Debug, Clone)]
pub struct RootCandidate {
    pub kind: KnownFolderKind,
    pub path: PathBuf,
}

pub fn known_folder_roots(project_roots: &[PathBuf]) -> Result<Vec<RootCandidate>, NativeError> {
    let _apartment = ComApartment::sta()?;
    let mut roots = vec![
        known_folder(KnownFolderKind::Desktop, &FOLDERID_Desktop)?,
        known_folder(KnownFolderKind::Documents, &FOLDERID_Documents)?,
        known_folder(KnownFolderKind::Downloads, &FOLDERID_Downloads)?,
        known_folder(KnownFolderKind::Pictures, &FOLDERID_Pictures)?,
        known_folder(KnownFolderKind::Music, &FOLDERID_Music)?,
        known_folder(KnownFolderKind::Videos, &FOLDERID_Videos)?,
    ];
    roots.extend(project_roots.iter().cloned().map(|path| RootCandidate {
        kind: KnownFolderKind::Project,
        path,
    }));
    Ok(roots)
}

fn known_folder(kind: KnownFolderKind, id: &GUID) -> Result<RootCandidate, NativeError> {
    // SAFETY: id points to a static known-folder GUID and the returned allocation is freed below.
    let value = unsafe { SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None) }?;
    // SAFETY: SHGetKnownFolderPath returns a NUL-terminated CoTaskMem allocation.
    let path = PathBuf::from(unsafe { take_co_task_mem_string(value) });
    Ok(RootCandidate { kind, path })
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct RootAttributes {
    pub hidden: bool,
    pub system: bool,
    pub reparse: bool,
    pub offline: bool,
    pub recall_on_open: bool,
    pub recall_on_data_access: bool,
}

impl RootAttributes {
    #[must_use]
    pub const fn content_requires_hydration(self) -> bool {
        self.offline || self.recall_on_open || self.recall_on_data_access
    }
}

pub fn root_attributes(path: &Path) -> Result<RootAttributes, NativeError> {
    let metadata = std::fs::symlink_metadata(path)?;
    let value = metadata.file_attributes();
    Ok(RootAttributes {
        hidden: value & FILE_ATTRIBUTE_HIDDEN.0 != 0,
        system: value & FILE_ATTRIBUTE_SYSTEM.0 != 0,
        reparse: value & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0,
        offline: value & FILE_ATTRIBUTE_OFFLINE.0 != 0,
        recall_on_open: value & FILE_ATTRIBUTE_RECALL_ON_OPEN.0 != 0,
        recall_on_data_access: value & FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS.0 != 0,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StableFileIdentity {
    pub volume_serial: u64,
    pub file_id: [u8; 16],
}

pub fn file_identity(path: &Path) -> Result<(StableFileIdentity, bool), NativeError> {
    let wide = nul_terminated(path.as_os_str())?;
    // SAFETY: wide is NUL-terminated; access and sharing flags request metadata only.
    let handle = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            FILE_READ_ATTRIBUTES.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            None,
        )
    }?;
    let handle = OwnedHandle::new(handle)?;
    // SAFETY: the output buffer has the exact FILE_ID_INFO representation and size.
    let mut info = unsafe { zeroed::<FILE_ID_INFO>() };
    // SAFETY: handle is valid and info is writable for the supplied size.
    unsafe {
        GetFileInformationByHandleEx(
            handle.raw(),
            FileIdInfo,
            (&raw mut info).cast::<c_void>(),
            u32::try_from(size_of::<FILE_ID_INFO>()).map_err(|_| NativeError::SizeOverflow)?,
        )
    }?;

    let mut filesystem = [0u16; 32];
    // SAFETY: handle names a file on the volume and every optional output buffer is valid.
    unsafe {
        GetVolumeInformationByHandleW(handle.raw(), None, None, None, None, Some(&mut filesystem))
    }?;
    let filesystem = OsString::from_wide(nul_prefix(&filesystem));
    Ok((
        StableFileIdentity {
            volume_serial: info.VolumeSerialNumber,
            file_id: info.FileId.Identifier,
        },
        filesystem.eq_ignore_ascii_case("NTFS"),
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ShellGeneration(NonZeroU64);

impl ShellGeneration {
    #[must_use]
    pub const fn new(value: NonZeroU64) -> Self {
        Self(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ShellItemToken {
    generation: ShellGeneration,
    slot: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShellActivationKind {
    Packaged,
    Classic,
}

#[derive(Debug, Clone)]
pub struct ShellCatalogItem {
    pub token: ShellItemToken,
    pub display: OsString,
    pub activation: ShellActivationKind,
    identity: OsString,
}

#[derive(Debug, Clone)]
pub struct ShellCatalogSnapshot {
    generation: ShellGeneration,
    pub items: Vec<ShellCatalogItem>,
    pub invalid_utf16_display_count: usize,
}

impl ShellCatalogSnapshot {
    #[must_use]
    pub fn resolve(&self, token: ShellItemToken) -> Option<&ShellCatalogItem> {
        if token.generation != self.generation {
            return None;
        }
        usize::try_from(token.slot)
            .ok()
            .and_then(|slot| self.items.get(slot))
    }

    #[must_use]
    pub fn identity_overlap(&self, other: &Self) -> usize {
        self.items
            .iter()
            .filter(|left| {
                other
                    .items
                    .iter()
                    .any(|right| right.identity == left.identity)
            })
            .count()
    }
}

pub fn enumerate_apps(generation: ShellGeneration) -> Result<ShellCatalogSnapshot, NativeError> {
    let _apartment = ComApartment::sta()?;
    // SAFETY: the known-folder GUID is static and the requested interface matches T.
    let folder: IShellItem =
        unsafe { SHGetKnownFolderItem(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None) }?;
    // SAFETY: BHID_EnumItems is the documented enumeration handler for shell items.
    let enumerator: IEnumShellItems = unsafe { folder.BindToHandler(None, &BHID_EnumItems) }?;
    let mut items = Vec::new();
    let mut invalid_utf16_display_count = 0usize;

    loop {
        let mut next = [None];
        let mut fetched = 0u32;
        // SAFETY: next has capacity for one interface and fetched is a valid output pointer.
        // S_FALSE is represented as success with zero fetched; every real error propagates.
        unsafe { enumerator.Next(&mut next, Some(&raw mut fetched)) }?;
        if fetched == 0 {
            break;
        }
        let Some(item) = next.into_iter().next().flatten() else {
            return Err(NativeError::ShellEnumerationContract);
        };
        // SAFETY: Shell owns both returned CoTaskMem strings; the helper frees them.
        let display = unsafe { take_co_task_mem_string(item.GetDisplayName(SIGDN_NORMALDISPLAY)?) };
        // SAFETY: same allocation contract as the display name.
        let identity =
            unsafe { take_co_task_mem_string(item.GetDisplayName(SIGDN_DESKTOPABSOLUTEPARSING)?) };
        let activation = if app_user_model_id(&item)?.is_some() {
            ShellActivationKind::Packaged
        } else {
            ShellActivationKind::Classic
        };
        if display.to_str().is_none() {
            invalid_utf16_display_count = invalid_utf16_display_count
                .checked_add(1)
                .ok_or(NativeError::SizeOverflow)?;
        }
        let slot = u32::try_from(items.len()).map_err(|_| NativeError::SizeOverflow)?;
        items.push(ShellCatalogItem {
            token: ShellItemToken { generation, slot },
            display,
            activation,
            identity,
        });
    }

    Ok(ShellCatalogSnapshot {
        generation,
        items,
        invalid_utf16_display_count,
    })
}

fn app_user_model_id(item: &IShellItem) -> Result<Option<OsString>, NativeError> {
    let item: IShellItem2 = item.cast()?;
    // SAFETY: property store is requested read-only and tied to the shell item lifetime.
    let store: IPropertyStore = unsafe { item.GetPropertyStore(GPS_DEFAULT) }?;
    // SAFETY: key is a static PROPERTYKEY and the return value is initialized by COM.
    let mut value: PROPVARIANT = unsafe { store.GetValue(&PKEY_APP_USER_MODEL_ID) }?;
    // SAFETY: value is an initialized PROPVARIANT and the returned string uses CoTaskMem.
    let text = unsafe { PropVariantToStringAlloc(&raw const value) };
    let converted = text
        .ok()
        // SAFETY: PropVariantToStringAlloc returns a NUL-terminated CoTaskMem allocation.
        .map(|text| unsafe { take_co_task_mem_string(text) })
        .filter(|value| !value.is_empty());
    // SAFETY: value was initialized by GetValue and is cleared once before leaving.
    unsafe { PropVariantClear(&raw mut value) }?;
    Ok(converted)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellEnumerationMeasurement {
    pub first_count: usize,
    pub second_count: usize,
    pub identity_overlap: usize,
    pub packaged_count: usize,
    pub classic_count: usize,
    pub invalid_utf16_display_count: usize,
    pub empty_display_count: usize,
    pub packaged_activation_route_available: bool,
    pub stale_token_rejected: bool,
    pub refresh_ns: u64,
}

pub fn measure_shell_enumeration()
-> Result<(ShellCatalogSnapshot, ShellEnumerationMeasurement), NativeError> {
    let first = enumerate_apps(ShellGeneration::new(NonZeroU64::MIN))?;
    let stale = first.items.first().map(|item| item.token);
    let started = Instant::now();
    let second = enumerate_apps(ShellGeneration::new(
        NonZeroU64::new(2).ok_or(NativeError::SizeOverflow)?,
    ))?;
    let refresh_ns = nanos(started.elapsed())?;
    let stale_token_rejected = stale.is_none_or(|token| second.resolve(token).is_none());
    let packaged_count = second
        .items
        .iter()
        .filter(|item| item.activation == ShellActivationKind::Packaged)
        .count();
    let classic_count = second.items.len().saturating_sub(packaged_count);
    let measurement = ShellEnumerationMeasurement {
        first_count: first.items.len(),
        second_count: second.items.len(),
        identity_overlap: first.identity_overlap(&second),
        packaged_count,
        classic_count,
        invalid_utf16_display_count: second.invalid_utf16_display_count,
        empty_display_count: second
            .items
            .iter()
            .filter(|item| item.display.is_empty())
            .count(),
        packaged_activation_route_available: packaged_activation_route_available()?,
        stale_token_rejected,
        refresh_ns,
    };
    Ok((second, measurement))
}

fn packaged_activation_route_available() -> Result<bool, NativeError> {
    let _apartment = ComApartment::sta()?;
    // SAFETY: this creates only the documented in-process activation-manager interface and does
    // not call an activation method or launch an owner-installed application.
    let _: IApplicationActivationManager =
        unsafe { CoCreateInstance(&ApplicationActivationManager, None, CLSCTX_INPROC_SERVER) }?;
    Ok(true)
}

pub fn create_shortcut(path: &Path, target: &Path, arguments: &OsStr) -> Result<(), NativeError> {
    let _apartment = ComApartment::sta()?;
    // SAFETY: ShellLink is an in-proc COM class and the requested interface matches.
    let link: IShellLinkW = unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }?;
    let target = nul_terminated(target.as_os_str())?;
    let arguments = nul_terminated(arguments)?;
    // SAFETY: both strings are valid NUL-terminated UTF-16 for the duration of the calls.
    unsafe {
        link.SetPath(PCWSTR(target.as_ptr()))?;
        link.SetArguments(PCWSTR(arguments.as_ptr()))?;
    }
    let persisted: IPersistFile = link.cast()?;
    let path = nul_terminated(path.as_os_str())?;
    // SAFETY: path is valid NUL-terminated UTF-16 and the object owns its save operation.
    unsafe { persisted.Save(PCWSTR(path.as_ptr()), true) }?;
    Ok(())
}

pub fn shortcut_arguments(path: &Path) -> Result<OsString, NativeError> {
    let _apartment = ComApartment::sta()?;
    // SAFETY: ShellLink is an in-proc COM class and the requested interface matches.
    let link: IShellLinkW = unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }?;
    let persisted: IPersistFile = link.cast()?;
    let path = nul_terminated(path.as_os_str())?;
    // SAFETY: path is valid NUL-terminated UTF-16 and STGM_READ is represented by zero.
    unsafe { persisted.Load(PCWSTR(path.as_ptr()), STGM(0)) }?;
    let mut arguments = [0u16; 1024];
    // SAFETY: the output buffer is writable and its length is exact.
    unsafe { link.GetArguments(&mut arguments) }?;
    Ok(OsString::from_wide(nul_prefix(&arguments)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessBirth(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapturedWindowIdentity {
    hwnd: usize,
    process_id: NonZeroU32,
    birth: ProcessBirth,
}

pub fn capture_foreground_window() -> Result<Option<CapturedWindowIdentity>, NativeError> {
    // SAFETY: GetForegroundWindow has no preconditions.
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        return Ok(None);
    }
    let mut process_id = 0u32;
    // SAFETY: hwnd came from Windows and process_id is writable.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&raw mut process_id)) };
    let process_id = NonZeroU32::new(process_id).ok_or(NativeError::WindowHasNoProcess)?;
    Ok(Some(CapturedWindowIdentity {
        hwnd: hwnd.0.addr(),
        process_id,
        birth: process_birth(process_id)?,
    }))
}

pub fn revalidate_and_activate_once(
    identity: CapturedWindowIdentity,
    attempts: &mut u32,
) -> Result<bool, NativeError> {
    let hwnd = HWND(std::ptr::with_exposed_provenance_mut::<c_void>(
        identity.hwnd,
    ));
    // SAFETY: IsWindow validates the raw HWND before further use.
    if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        return Ok(false);
    }
    let mut process_id = 0u32;
    // SAFETY: hwnd passed IsWindow and process_id is writable.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&raw mut process_id)) };
    if NonZeroU32::new(process_id) != Some(identity.process_id)
        || process_birth(identity.process_id)? != identity.birth
    {
        return Ok(false);
    }
    *attempts = attempts.checked_add(1).ok_or(NativeError::SizeOverflow)?;
    // SAFETY: hwnd was fully revalidated and the contract permits one foreground attempt.
    Ok(unsafe { SetForegroundWindow(hwnd) }.as_bool())
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CapturedActivationMeasurement {
    pub foreground_was_present: bool,
    pub identity_revalidated: bool,
    pub foreground_attempts: u32,
}

pub fn measure_captured_activation() -> Result<CapturedActivationMeasurement, NativeError> {
    let Some(identity) = capture_foreground_window()? else {
        return Ok(CapturedActivationMeasurement {
            foreground_was_present: false,
            identity_revalidated: false,
            foreground_attempts: 0,
        });
    };
    let mut attempts = 0;
    let identity_revalidated = revalidate_and_activate_once(identity, &mut attempts)?;
    Ok(CapturedActivationMeasurement {
        foreground_was_present: true,
        identity_revalidated,
        foreground_attempts: attempts,
    })
}

fn process_birth(process_id: NonZeroU32) -> Result<ProcessBirth, NativeError> {
    // SAFETY: the PID is nonzero and the requested right is query-only.
    let handle =
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id.get()) }?;
    let handle = OwnedHandle::new(handle)?;
    // SAFETY: handle is queryable and all FILETIME outputs are writable.
    let (mut creation, mut exit, mut kernel, mut user) = unsafe {
        (
            zeroed::<FILETIME>(),
            zeroed::<FILETIME>(),
            zeroed::<FILETIME>(),
            zeroed::<FILETIME>(),
        )
    };
    // SAFETY: every pointer above is valid for one FILETIME.
    unsafe {
        GetProcessTimes(
            handle.raw(),
            &raw mut creation,
            &raw mut exit,
            &raw mut kernel,
            &raw mut user,
        )
    }?;
    Ok(ProcessBirth(filetime_u64(creation)))
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ProcessCounters {
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub other_bytes: u64,
    pub kernel_100ns: u64,
    pub user_100ns: u64,
}

pub fn current_process_counters() -> Result<ProcessCounters, NativeError> {
    // SAFETY: pseudo handle is always valid in the current process.
    let process = unsafe { GetCurrentProcess() };
    process_counters(process)
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[allow(clippy::struct_field_names)]
pub struct ProcessMemory {
    pub working_set_bytes: usize,
    pub private_bytes: usize,
    pub peak_working_set_bytes: usize,
}

pub fn current_process_memory() -> Result<ProcessMemory, NativeError> {
    // SAFETY: GetCurrentProcess always returns a valid pseudo handle.
    let process = unsafe { GetCurrentProcess() };
    process_memory(process)
}

pub fn process_memory(process: HANDLE) -> Result<ProcessMemory, NativeError> {
    // SAFETY: zero initialization is valid before GetProcessMemoryInfo fills this C structure.
    let mut counters = unsafe { zeroed::<PROCESS_MEMORY_COUNTERS_EX>() };
    counters.cb = u32::try_from(size_of::<PROCESS_MEMORY_COUNTERS_EX>())
        .map_err(|_| NativeError::SizeOverflow)?;
    // SAFETY: the pseudo handle is valid and the extended structure begins with the documented
    // PROCESS_MEMORY_COUNTERS layout accepted by this function.
    unsafe {
        GetProcessMemoryInfo(
            process,
            (&raw mut counters).cast::<PROCESS_MEMORY_COUNTERS>(),
            counters.cb,
        )
    }?;
    Ok(ProcessMemory {
        working_set_bytes: counters.WorkingSetSize,
        private_bytes: counters.PrivateUsage,
        peak_working_set_bytes: counters.PeakWorkingSetSize,
    })
}

pub fn process_counters(process: HANDLE) -> Result<ProcessCounters, NativeError> {
    // SAFETY: caller supplies a queryable process handle; every output is writable.
    let mut io = unsafe { zeroed::<IO_COUNTERS>() };
    // SAFETY: io has the documented representation and size.
    unsafe { GetProcessIoCounters(process, &raw mut io) }?;
    // SAFETY: all four FILETIME outputs are initialized by GetProcessTimes.
    let (mut creation, mut exit, mut kernel, mut user) = unsafe {
        (
            zeroed::<FILETIME>(),
            zeroed::<FILETIME>(),
            zeroed::<FILETIME>(),
            zeroed::<FILETIME>(),
        )
    };
    // SAFETY: process is queryable and every output pointer is valid.
    unsafe {
        GetProcessTimes(
            process,
            &raw mut creation,
            &raw mut exit,
            &raw mut kernel,
            &raw mut user,
        )
    }?;
    Ok(ProcessCounters {
        read_bytes: io.ReadTransferCount,
        write_bytes: io.WriteTransferCount,
        other_bytes: io.OtherTransferCount,
        kernel_100ns: filetime_u64(kernel),
        user_100ns: filetime_u64(user),
    })
}

pub fn activation_probe(event_name: &OsStr) -> Result<(), NativeError> {
    let event_name = nul_terminated(event_name)?;
    // SAFETY: name is valid NUL-terminated UTF-16 and the access right is exact.
    let event = unsafe { OpenEventW(EVENT_MODIFY_STATE, false, PCWSTR(event_name.as_ptr())) }?;
    let event = OwnedHandle::new(event)?;
    // SAFETY: event is a valid event handle opened with EVENT_MODIFY_STATE.
    unsafe { SetEvent(event.raw()) }?;
    Ok(())
}

pub fn shell_execute_hidden(executable: &Path, arguments: &OsStr) -> Result<u64, NativeError> {
    let executable = nul_terminated(executable.as_os_str())?;
    let arguments = nul_terminated(arguments)?;
    let verb = nul_terminated(OsStr::new("open"))?;
    // SAFETY: zero is the documented initialization for SHELLEXECUTEINFOW before fields are set.
    let mut info = unsafe { zeroed::<SHELLEXECUTEINFOW>() };
    info.cbSize =
        u32::try_from(size_of::<SHELLEXECUTEINFOW>()).map_err(|_| NativeError::SizeOverflow)?;
    info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_FLAG_NO_UI | SEE_MASK_ASYNCOK;
    info.lpVerb = PCWSTR(verb.as_ptr());
    info.lpFile = PCWSTR(executable.as_ptr());
    info.lpParameters = PCWSTR(arguments.as_ptr());
    info.nShow = SW_HIDE.0;
    let started = Instant::now();
    // SAFETY: info contains valid pointers for the duration of the synchronous handoff call.
    unsafe { ShellExecuteExW(&raw mut info) }?;
    let elapsed = nanos(started.elapsed())?;
    if !info.hProcess.is_invalid() {
        let _process = OwnedHandle::new(info.hProcess)?;
    }
    Ok(elapsed)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassicActivationMeasurement {
    pub duplicate_target_arguments_distinct: bool,
    pub shortcut_arguments_round_trip: bool,
    pub shell_handoff_ns: Vec<u64>,
    pub probe_signaled_count: usize,
}

pub fn measure_classic_activation(
    executable: &Path,
    fixture_root: &Path,
) -> Result<ClassicActivationMeasurement, NativeError> {
    let first = fixture_root.join("activation-a.lnk");
    let second = fixture_root.join("activation-b.lnk");
    let first_arguments = OsString::from("--profile first");
    let second_arguments = OsString::from("--profile second");
    create_shortcut(&first, executable, &first_arguments)?;
    create_shortcut(&second, executable, &second_arguments)?;
    let first_read = shortcut_arguments(&first)?;
    let second_read = shortcut_arguments(&second)?;
    let mut shell_handoff_ns = Vec::with_capacity(24);
    let mut probe_signaled_count = 0usize;
    for sample in 0..24 {
        let event_name = format!("Local\\wayfinder-palette-{}", uuid::Uuid::new_v4());
        let event_name_wide = nul_terminated(OsStr::new(&event_name))?;
        // SAFETY: the name is valid NUL-terminated UTF-16; the event is local and initially reset.
        let event = unsafe { CreateEventW(None, false, false, PCWSTR(event_name_wide.as_ptr())) }?;
        let event = OwnedHandle::new(event)?;
        let shortcut = fixture_root.join(format!("activation-probe-{sample:02}.lnk"));
        let arguments = OsString::from(format!(
            "activation-probe --event \"{event_name}\" --profile probe-{sample:02}"
        ));
        create_shortcut(&shortcut, executable, &arguments)?;
        shell_handoff_ns.push(shell_execute_hidden(&shortcut, OsStr::new(""))?);
        // SAFETY: event is valid and this is one bounded kernel wait, not a polling loop.
        if unsafe { WaitForSingleObject(event.raw(), 5_000) } == WAIT_OBJECT_0 {
            probe_signaled_count = probe_signaled_count
                .checked_add(1)
                .ok_or(NativeError::SizeOverflow)?;
        }
    }
    Ok(ClassicActivationMeasurement {
        duplicate_target_arguments_distinct: first_read != second_read,
        shortcut_arguments_round_trip: first_read == first_arguments
            && second_read == second_arguments,
        shell_handoff_ns,
        probe_signaled_count,
    })
}

unsafe fn take_co_task_mem_string(value: PWSTR) -> OsString {
    let pointer = PCWSTR(value.0);
    // SAFETY: caller guarantees a NUL-terminated CoTaskMem string.
    let text = unsafe { pointer.as_wide() };
    let result = OsString::from_wide(text);
    // SAFETY: caller transfers ownership of exactly one CoTaskMem allocation.
    unsafe { CoTaskMemFree(Some(value.0.cast::<c_void>())) };
    result
}

fn nul_terminated(value: &OsStr) -> Result<Vec<u16>, NativeError> {
    let mut wide = value.encode_wide().collect::<Vec<_>>();
    if wide.contains(&0) {
        return Err(NativeError::InteriorNul);
    }
    wide.push(0);
    Ok(wide)
}

fn nul_prefix(value: &[u16]) -> &[u16] {
    match value.iter().position(|unit| *unit == 0) {
        Some(length) => value.get(..length).unwrap_or(value),
        None => value,
    }
}

const fn filetime_u64(value: FILETIME) -> u64 {
    (value.dwHighDateTime as u64) << 32 | value.dwLowDateTime as u64
}

fn nanos(duration: Duration) -> Result<u64, NativeError> {
    u64::try_from(duration.as_nanos()).map_err(|_| NativeError::DurationOverflow)
}

#[derive(Debug, Error)]
pub enum NativeError {
    #[error("Windows API rejected the operation")]
    Windows(#[from] windows::core::Error),
    #[error("filesystem operation failed")]
    Io(#[from] std::io::Error),
    #[error("native handle is invalid")]
    InvalidHandle,
    #[error("native string contains an interior NUL")]
    InteriorNul,
    #[error("duration does not fit the report representation")]
    DurationOverflow,
    #[error("size does not fit the native or report representation")]
    SizeOverflow,
    #[error("Shell enumeration violated its output contract")]
    ShellEnumerationContract,
    #[error("captured window has no owning process")]
    WindowHasNoProcess,
}
