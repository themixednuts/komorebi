use std::ffi::c_void;
use std::io::BufRead;
use std::io::Write;
use std::mem::size_of;
use std::sync::Barrier;
use std::sync::OnceLock;
use std::sync::atomic::AtomicIsize;
use std::sync::atomic::Ordering;
use std::thread;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use serde::Deserialize;
use serde::Serialize;
use windows::Win32::Foundation::HINSTANCE;
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::LPARAM;
use windows::Win32::Foundation::LRESULT;
use windows::Win32::Foundation::WPARAM;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Accessibility::NotifyWinEvent;
use windows::Win32::UI::Accessibility::UiaRootObjectId;
use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows::Win32::UI::WindowsAndMessaging::CreateWindowExW;
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;
use windows::Win32::UI::WindowsAndMessaging::DestroyWindow;
use windows::Win32::UI::WindowsAndMessaging::DispatchMessageW;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_NAMECHANGE;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_SHOW;
use windows::Win32::UI::WindowsAndMessaging::GetMessageW;
use windows::Win32::UI::WindowsAndMessaging::MSG;
use windows::Win32::UI::WindowsAndMessaging::OBJID_WINDOW;
use windows::Win32::UI::WindowsAndMessaging::PostQuitMessage;
use windows::Win32::UI::WindowsAndMessaging::PostThreadMessageW;
use windows::Win32::UI::WindowsAndMessaging::RegisterClassExW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNOACTIVATE;
use windows::Win32::UI::WindowsAndMessaging::ShowWindow;
use windows::Win32::UI::WindowsAndMessaging::TranslateMessage;
use windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE;
use windows::Win32::UI::WindowsAndMessaging::WM_APP;
use windows::Win32::UI::WindowsAndMessaging::WM_GETOBJECT;
use windows::Win32::UI::WindowsAndMessaging::WNDCLASSEXW;
use windows::Win32::UI::WindowsAndMessaging::WS_CAPTION;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_DLGMODALFRAME;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_NOACTIVATE;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_TOOLWINDOW;
use windows::Win32::UI::WindowsAndMessaging::WS_OVERLAPPEDWINDOW;
use windows::Win32::UI::WindowsAndMessaging::WS_POPUP;
use windows::Win32::UI::WindowsAndMessaging::WS_SYSMENU;
use windows::core::PCWSTR;

const COMMAND_STORM: u32 = WM_APP + 1;
const COMMAND_QUIT: u32 = WM_APP + 2;
static HUNG_WINDOW: AtomicIsize = AtomicIsize::new(0);
static HUNG_PROVIDER_BARRIER: OnceLock<Barrier> = OnceLock::new();

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FixtureWindow {
    pub role: String,
    pub window: isize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProducerManifest {
    pub process_id: u32,
    pub windows: Vec<FixtureWindow>,
}

pub fn run() -> Result<()> {
    HUNG_PROVIDER_BARRIER.get_or_init(|| Barrier::new(2));
    await_create_command()?;
    // SAFETY: This call has no pointers and identifies the current producer thread.
    let main_thread = unsafe { GetCurrentThreadId() };
    // SAFETY: None requests the already-loaded current executable module.
    let module = unsafe { GetModuleHandleW(None) }?;
    let instance = HINSTANCE(module.0);
    let specs = fixture_specs();
    let mut class_storage = Vec::with_capacity(specs.len());
    let mut windows = Vec::with_capacity(specs.len());
    let mut created_by_role = Vec::<(&'static str, HWND)>::with_capacity(specs.len());
    for spec in &specs {
        let class = nul_terminated(spec.class);
        register_class(instance, &class)?;
        let owner = spec
            .owner_role
            .map(|owner_role| {
                created_by_role
                    .iter()
                    .find(|(role, _)| *role == owner_role)
                    .map(|(_, window)| *window)
                    .with_context(|| format!("fixture owner {owner_role} must be created first"))
            })
            .transpose()?;
        let window = create_fixture(instance, &class, spec, owner)?;
        created_by_role.push((spec.role, window));
        if spec.role == "hung_provider" {
            HUNG_WINDOW.store(raw_window(window), Ordering::Release);
        }
        // SAFETY: `window` was just created on this thread and the show command is valid.
        unsafe {
            let _was_visible = ShowWindow(
                window,
                if spec.no_activate {
                    SW_SHOWNOACTIVATE
                } else {
                    SW_SHOW
                },
            );
        }
        windows.push(FixtureWindow {
            role: spec.role.to_owned(),
            window: raw_window(window),
        });
        class_storage.push(class);
    }
    let root = created_by_role
        .iter()
        .find(|(role, _)| *role == "root")
        .map(|(_, window)| *window)
        .context("fixture root was not created")?;
    // SAFETY: `root` was created on this thread and remains live for the producer lifetime.
    unsafe {
        let _was_enabled = EnableWindow(root, false);
    }
    let manifest = ProducerManifest {
        process_id: std::process::id(),
        windows,
    };
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, &manifest)?;
    stdout.write_all(b"\n")?;
    stdout.flush()?;
    drop(stdout);
    spawn_command_reader(main_thread)?;
    message_loop(&manifest)
}

fn await_create_command() -> Result<()> {
    let mut command = Vec::new();
    std::io::stdin().lock().read_until(b'\n', &mut command)?;
    if trim_ascii(&command) != b"create" {
        bail!("producer expected the create command");
    }
    Ok(())
}

fn spawn_command_reader(thread_id: u32) -> Result<()> {
    thread::Builder::new()
        .name("popup-producer-commands".to_owned())
        .spawn(move || {
            let mut input = std::io::stdin().lock();
            let mut command = Vec::new();
            loop {
                command.clear();
                let Ok(count) = input.read_until(b'\n', &mut command) else {
                    let _cleanup_signal = post_command(thread_id, COMMAND_QUIT, 0);
                    break;
                };
                if count == 0 {
                    if post_command(thread_id, COMMAND_QUIT, 0).is_err() {
                        break;
                    }
                    break;
                }
                let command = trim_ascii(&command);
                if command == b"quit" {
                    if post_command(thread_id, COMMAND_QUIT, 0).is_err() {
                        break;
                    }
                    break;
                }
                if let Some(raw_count) = command.strip_prefix(b"storm:") {
                    let Ok(count) = parse_storm_count(raw_count) else {
                        let _cleanup_signal = post_command(thread_id, COMMAND_QUIT, 0);
                        break;
                    };
                    if post_command(thread_id, COMMAND_STORM, count).is_err() {
                        break;
                    }
                }
            }
        })?;
    Ok(())
}

fn parse_storm_count(raw: &[u8]) -> Result<usize> {
    Ok(std::str::from_utf8(raw)?.parse()?)
}

fn post_command(thread_id: u32, command: u32, value: usize) -> windows::core::Result<()> {
    // SAFETY: The target is the live producer thread; command/value contain no borrowed pointers.
    unsafe { PostThreadMessageW(thread_id, command, WPARAM(value), LPARAM(0)) }
}

fn message_loop(manifest: &ProducerManifest) -> Result<()> {
    let root = manifest
        .windows
        .first()
        .map(|fixture| hwnd_from_raw(fixture.window))
        .context("fixture manifest has no root")?;
    let mut message = MSG::default();
    loop {
        // SAFETY: `message` is valid writable storage owned by this message-loop thread.
        let status = unsafe { GetMessageW(&raw mut message, None, 0, 0) };
        if status.0 == -1 {
            return Err(windows::core::Error::from_thread().into());
        }
        if status.0 == 0 {
            return Ok(());
        }
        match message.message {
            COMMAND_STORM => {
                for _ in 0..message.wParam.0 {
                    // SAFETY: `root` remains live and the event/object pair is documented.
                    unsafe {
                        NotifyWinEvent(EVENT_OBJECT_SHOW, root, OBJID_WINDOW.0, 0);
                    }
                }
                // SAFETY: `root` remains live and the name-change event/object pair is documented.
                unsafe {
                    NotifyWinEvent(EVENT_OBJECT_NAMECHANGE, root, OBJID_WINDOW.0, 0);
                }
                emit_signal(b"storm_complete\n")?;
            }
            COMMAND_QUIT => {
                for fixture in manifest.windows.iter().rev() {
                    let window = hwnd_from_raw(fixture.window);
                    // SAFETY: IsWindow accepts arbitrary HWND values and validates them internally.
                    if unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindow(Some(window)) }
                        .as_bool()
                    {
                        // SAFETY: The window is live and was created by this producer thread.
                        unsafe { DestroyWindow(window) }?;
                    }
                }
                // SAFETY: Called on the producer's own message-loop thread.
                unsafe { PostQuitMessage(0) };
            }
            _ => {
                // SAFETY: GetMessageW initialized `message`; it remains alive for both calls.
                unsafe {
                    // A false result only means this message needs no keyboard translation.
                    let _ = TranslateMessage(&raw const message);
                    DispatchMessageW(&raw const message);
                }
            }
        }
    }
}

fn emit_signal(signal: &[u8]) -> Result<()> {
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(signal)?;
    stdout.flush()?;
    Ok(())
}

#[derive(Clone, Copy)]
struct FixtureSpec {
    role: &'static str,
    class: &'static str,
    owner_role: Option<&'static str>,
    ex_style: WINDOW_EX_STYLE,
    no_activate: bool,
}

fn fixture_specs() -> [FixtureSpec; 11] {
    [
        FixtureSpec::root("root", "Wayfinder.Root"),
        FixtureSpec::owned_by(
            "modal_dialog",
            "Wayfinder.Modal",
            "root",
            WS_EX_DLGMODALFRAME,
            false,
        ),
        FixtureSpec::root("modeless_root", "Wayfinder.ModelessRoot"),
        FixtureSpec::owned_by(
            "modeless_dialog",
            "Wayfinder.Modeless",
            "modeless_root",
            WS_EX_DLGMODALFRAME,
            false,
        ),
        FixtureSpec::owned_by(
            "utility",
            "Wayfinder.Utility",
            "modeless_root",
            WS_EX_TOOLWINDOW,
            false,
        ),
        FixtureSpec::owned_by(
            "no_activate",
            "Wayfinder.NoActivate",
            "modeless_root",
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            true,
        ),
        FixtureSpec::owned_by(
            "menu",
            "Wayfinder.Menu",
            "modeless_root",
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            true,
        ),
        FixtureSpec::owned_by(
            "tooltip",
            "Wayfinder.Tooltip",
            "modeless_root",
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            true,
        ),
        FixtureSpec::owned_by(
            "combo_popup",
            "Wayfinder.Combo",
            "modeless_root",
            WS_EX_TOOLWINDOW,
            false,
        ),
        FixtureSpec::owned_by(
            "drag_visual",
            "Wayfinder.Drag",
            "modeless_root",
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            true,
        ),
        FixtureSpec::owned_by(
            "hung_provider",
            "Wayfinder.Hung",
            "modeless_root",
            WS_EX_DLGMODALFRAME,
            false,
        ),
    ]
}

impl FixtureSpec {
    const fn root(role: &'static str, class: &'static str) -> Self {
        Self {
            role,
            class,
            owner_role: None,
            ex_style: WINDOW_EX_STYLE(0),
            no_activate: false,
        }
    }

    const fn owned_by(
        role: &'static str,
        class: &'static str,
        owner_role: &'static str,
        ex_style: WINDOW_EX_STYLE,
        no_activate: bool,
    ) -> Self {
        Self {
            role,
            class,
            owner_role: Some(owner_role),
            ex_style,
            no_activate,
        }
    }
}

fn register_class(instance: HINSTANCE, class: &[u16]) -> Result<()> {
    let descriptor = WNDCLASSEXW {
        cbSize: u32::try_from(size_of::<WNDCLASSEXW>())?,
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        lpszClassName: PCWSTR(class.as_ptr()),
        ..Default::default()
    };
    // SAFETY: The descriptor and its class-name backing storage remain valid for the call.
    let atom = unsafe { RegisterClassExW(&raw const descriptor) };
    if atom == 0 {
        return Err(windows::core::Error::from_thread().into());
    }
    Ok(())
}

fn create_fixture(
    instance: HINSTANCE,
    class: &[u16],
    spec: &FixtureSpec,
    owner: Option<HWND>,
) -> Result<HWND> {
    let style = if spec.owner_role.is_none() {
        WS_OVERLAPPEDWINDOW
    } else {
        WS_POPUP | WS_CAPTION | WS_SYSMENU
    };
    // SAFETY: Class and title buffers are NUL-terminated, instance/class are registered, and the
    // optional owner was created on this same thread. No creation parameter is borrowed.
    unsafe {
        CreateWindowExW(
            spec.ex_style,
            PCWSTR(class.as_ptr()),
            PCWSTR(class.as_ptr()),
            style,
            120 + i32::from(spec.owner_role.is_some()) * 80,
            120 + i32::from(spec.owner_role.is_some()) * 60,
            if spec.owner_role.is_none() { 800 } else { 360 },
            if spec.owner_role.is_none() { 500 } else { 220 },
            owner,
            None,
            Some(instance),
            None,
        )
    }
    .map_err(Into::into)
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_GETOBJECT
        && lparam.0 == isize::try_from(UiaRootObjectId).unwrap_or(isize::MIN)
        && raw_window(window) == HUNG_WINDOW.load(Ordering::Acquire)
        && let Some(barrier) = HUNG_PROVIDER_BARRIER.get()
    {
        barrier.wait();
    }
    // SAFETY: User32 supplied all callback arguments to this ABI-compatible window procedure.
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

fn nul_terminated(value: &str) -> Vec<u16> {
    value.encode_utf16().chain([0]).collect()
}

fn trim_ascii(value: &[u8]) -> &[u8] {
    value.trim_ascii_end()
}

fn raw_window(window: HWND) -> isize {
    isize::try_from(window.0.addr()).unwrap_or(isize::MAX)
}

fn hwnd_from_raw(raw: isize) -> HWND {
    let address = usize::try_from(raw).unwrap_or(usize::MAX);
    HWND(std::ptr::with_exposed_provenance_mut::<c_void>(address))
}
