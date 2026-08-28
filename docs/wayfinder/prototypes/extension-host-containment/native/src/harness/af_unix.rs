use std::mem::size_of;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::ptr::null_mut;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use uuid::Uuid;
use windows_sys::Win32::Foundation::{HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::Networking::WinSock::{
    AF_UNIX, FD_ACCEPT, FD_ACCEPT_BIT, FIONBIO, INVALID_SOCKET, SO_RCVTIMEO, SO_SNDTIMEO,
    SOCK_STREAM, SOCKADDR, SOCKADDR_UN, SOCKET, SOCKET_ERROR, SOL_SOCKET, WSA_INVALID_EVENT,
    WSACleanup, WSACloseEvent, WSACreateEvent, WSADATA, WSAEVENT, WSAEnumNetworkEvents,
    WSAEventSelect, WSAGetLastError, WSANETWORKEVENTS, WSAStartup, accept, bind, closesocket,
    connect, ioctlsocket, listen, recv, send, setsockopt, socket,
};
use windows_sys::Win32::System::Threading::{
    CREATE_NO_WINDOW, GetExitCodeProcess, WaitForSingleObject,
};

use crate::protocol::ObservedOutcome;

use super::policy::ContainmentPolicy;
use super::report::{AfUnixEvidence, RunReport, Verification};

const WINSOCK_VERSION_2_2: u16 = 0x0202;
const ECHO_FRAME_BYTES: usize = size_of::<u64>();

pub(super) fn run_client(path: &Path, samples: usize, timeout: Duration) -> Result<()> {
    let _winsock = WinsockSession::start()?;
    let address = LocalSocketAddress::new(path)?;
    let socket = OwnedSocket::stream()?;
    set_socket_deadlines(&socket, timeout)?;
    connect_socket(&socket, &address)?;
    let mut frame = [0_u8; ECHO_FRAME_BYTES];
    for _ in 0..samples {
        receive_exact(&socket, &mut frame)?;
        send_all(&socket, &frame)?;
    }
    Ok(())
}

pub(super) fn run_comparison(
    host_executable: &Path,
    policy: &ContainmentPolicy,
    runs: &[RunReport],
) -> Result<AfUnixEvidence> {
    let lpac_socket_creation_denied = lpac_socket_creation_denied(runs)?;
    let _winsock = WinsockSession::start()?;
    let mut socket_path = TemporarySocketPath::new();
    let address = LocalSocketAddress::new(socket_path.path())?;
    let listener = OwnedSocket::stream()?;
    bind_socket(&listener, &address)?;
    listen_socket(&listener)?;
    let samples = policy.workload().echo_samples();
    let timeout = policy.pipe().operation_timeout();
    let child = ExternalChild::spawn(host_executable, socket_path.path(), samples, timeout)?;
    let connection = accept_with_deadline(&listener, timeout)?;
    set_blocking(&connection)?;
    set_socket_deadlines(&connection, timeout)?;

    let mut echo_rtt_us = Vec::with_capacity(samples);
    for sequence in 0..samples {
        let frame = u64::try_from(sequence)?.to_le_bytes();
        let started = Instant::now();
        send_all(&connection, &frame)?;
        let mut echoed = [0_u8; ECHO_FRAME_BYTES];
        receive_exact(&connection, &mut echoed)?;
        ensure!(echoed == frame, "AF_UNIX child echoed the wrong frame");
        echo_rtt_us.push(started.elapsed().as_secs_f64() * 1_000_000.0);
    }
    drop(connection);
    drop(listener);
    let child_exit_code = child.wait(timeout)?;
    ensure!(
        child_exit_code == 0,
        "AF_UNIX child exited {child_exit_code:#x}"
    );
    socket_path.remove()?;
    echo_rtt_us.sort_by(f64::total_cmp);

    Ok(AfUnixEvidence {
        role: "transport-only comparison; not an extension security boundary",
        endpoint_encoding: "ASCII probe path in narrow sockaddr_un.sun_path bytes; arbitrary WTF-16 is not representable",
        samples,
        full_trust_process_echo_p99_us: super::percentile(&echo_rtt_us, 99, 100),
        child_exit_code,
        endpoint_cleanup: Verification::Passed,
        lpac_socket_creation_denied: Verification::from(lpac_socket_creation_denied),
        kernel_peer_pid: "unavailable from public Windows AF_UNIX accept/getsockopt APIs",
        kernel_peer_token: "unavailable; no AppContainer SID binding equivalent to a pipe DACL",
    })
}

fn lpac_socket_creation_denied(runs: &[RunReport]) -> Result<bool> {
    ensure!(!runs.is_empty(), "AF_UNIX comparison requires LPAC runs");
    runs.iter()
        .map(|run| {
            let probe = run
                .probes
                .iter()
                .find(|probe| probe.name == "af_unix_socket_creation")
                .with_context(|| format!("missing AF_UNIX probe for {:?}", run.runtime))?;
            Ok(matches!(probe.observed, ObservedOutcome::Denied { .. }))
        })
        .collect::<Result<Vec<_>>>()
        .map(|outcomes| outcomes.into_iter().all(|denied| denied))
}

struct WinsockSession;

impl WinsockSession {
    fn start() -> Result<Self> {
        let mut data = WSADATA::default();
        // SAFETY: data is writable and version 2.2 is the documented Winsock request.
        let status = unsafe { WSAStartup(WINSOCK_VERSION_2_2, &raw mut data) };
        ensure!(status == 0, "initialize Winsock: error {status}");
        Ok(Self)
    }
}

impl Drop for WinsockSession {
    fn drop(&mut self) {
        // SAFETY: this balances one successful WSAStartup.
        if unsafe { WSACleanup() } == SOCKET_ERROR {
            eprintln!("failed to clean up Winsock: {}", last_winsock_error());
        }
    }
}

struct OwnedSocket(SOCKET);

impl OwnedSocket {
    fn stream() -> Result<Self> {
        // SAFETY: Winsock is initialized and these are the documented AF_UNIX stream parameters.
        let socket = unsafe { socket(i32::from(AF_UNIX), SOCK_STREAM, 0) };
        ensure!(
            socket != INVALID_SOCKET,
            "create AF_UNIX socket: {}",
            last_winsock_error()
        );
        Ok(Self(socket))
    }

    const fn raw(&self) -> SOCKET {
        self.0
    }
}

impl Drop for OwnedSocket {
    fn drop(&mut self) {
        // SAFETY: this value owns a valid socket and closes it exactly once.
        if unsafe { closesocket(self.0) } == SOCKET_ERROR {
            eprintln!("failed to close AF_UNIX socket: {}", last_winsock_error());
        }
    }
}

struct OwnedWsaEvent(WSAEVENT);

impl OwnedWsaEvent {
    fn create() -> Result<Self> {
        // SAFETY: Winsock is initialized.
        let event = unsafe { WSACreateEvent() };
        ensure!(
            event != WSA_INVALID_EVENT,
            "create Winsock event: {}",
            last_winsock_error()
        );
        Ok(Self(event))
    }

    fn handle(&self) -> HANDLE {
        self.0.cast_unsigned() as HANDLE
    }
}

impl Drop for OwnedWsaEvent {
    fn drop(&mut self) {
        // SAFETY: this value owns a valid Winsock event and closes it exactly once.
        if unsafe { WSACloseEvent(self.0) } == 0 {
            eprintln!("failed to close Winsock event: {}", last_winsock_error());
        }
    }
}

#[derive(Clone, Copy)]
struct LocalSocketAddress(SOCKADDR_UN);

impl LocalSocketAddress {
    fn new(path: &Path) -> Result<Self> {
        let text = path
            .to_str()
            .context("AF_UNIX sun_path cannot represent a WTF-16 endpoint")?;
        ensure!(text.is_ascii(), "test AF_UNIX endpoint must be ASCII");
        let bytes = text.as_bytes();
        ensure!(!bytes.contains(&0), "AF_UNIX endpoint contains NUL");
        ensure!(
            bytes.len() < SOCKADDR_UN::default().sun_path.len(),
            "AF_UNIX endpoint exceeds sun_path"
        );
        let mut address = SOCKADDR_UN {
            sun_family: AF_UNIX,
            ..Default::default()
        };
        for (destination, byte) in address.sun_path.iter_mut().zip(bytes.iter().copied()) {
            *destination = byte.cast_signed();
        }
        Ok(Self(address))
    }

    fn raw(&self) -> *const SOCKADDR {
        std::ptr::from_ref(&self.0).cast()
    }

    fn byte_len() -> Result<i32> {
        Ok(i32::try_from(size_of::<SOCKADDR_UN>())?)
    }
}

struct TemporarySocketPath {
    path: PathBuf,
    removed: bool,
}

impl TemporarySocketPath {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "komorebi-wayfinder-{}.sock",
            Uuid::new_v4().simple()
        ));
        Self {
            path,
            removed: false,
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn remove(&mut self) -> Result<()> {
        match std::fs::remove_file(&self.path) {
            Ok(()) => self.removed = true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => self.removed = true,
            Err(error) => return Err(error).context("remove AF_UNIX endpoint"),
        }
        Ok(())
    }
}

impl Drop for TemporarySocketPath {
    fn drop(&mut self) {
        if !self.removed
            && let Err(error) = self.remove()
        {
            eprintln!("failed to remove AF_UNIX endpoint: {error:#}");
        }
    }
}

struct ExternalChild(Option<Child>);

impl ExternalChild {
    fn spawn(executable: &Path, path: &Path, samples: usize, timeout: Duration) -> Result<Self> {
        let child = Command::new(executable)
            .arg("--af-unix-client")
            .arg(path.as_os_str())
            .arg(samples.to_string())
            .arg(timeout.as_millis().to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .context("spawn AF_UNIX comparison child")?;
        Ok(Self(Some(child)))
    }

    fn wait(mut self, timeout: Duration) -> Result<u32> {
        let child = self.0.as_mut().context("AF_UNIX child already reaped")?;
        // SAFETY: the Child owns this process handle and timeout is policy-bounded.
        let wait = unsafe {
            WaitForSingleObject(
                child.as_raw_handle().cast(),
                u32::try_from(timeout.as_millis())?,
            )
        };
        if wait == WAIT_TIMEOUT {
            bail!("timed out waiting for AF_UNIX child");
        }
        if wait != WAIT_OBJECT_0 {
            return Err(std::io::Error::last_os_error()).context("wait for AF_UNIX child");
        }
        let mut exit_code = 0_u32;
        // SAFETY: the process is signaled and exit_code is writable.
        if unsafe { GetExitCodeProcess(child.as_raw_handle().cast(), &raw mut exit_code) } == 0 {
            return Err(std::io::Error::last_os_error()).context("read AF_UNIX child exit code");
        }
        child.wait().context("reap AF_UNIX child")?;
        self.0.take();
        Ok(exit_code)
    }
}

impl Drop for ExternalChild {
    fn drop(&mut self) {
        let Some(child) = self.0.as_mut() else {
            return;
        };
        match child.try_wait() {
            Ok(Some(_)) => {}
            Ok(None) => {
                if let Err(error) = child.kill() {
                    eprintln!("failed to terminate AF_UNIX child: {error}");
                }
                if let Err(error) = child.wait() {
                    eprintln!("failed to reap AF_UNIX child: {error}");
                }
            }
            Err(error) => eprintln!("failed to query AF_UNIX child: {error}"),
        }
    }
}

fn bind_socket(socket: &OwnedSocket, address: &LocalSocketAddress) -> Result<()> {
    // SAFETY: address points to a fully initialized SOCKADDR_UN for the duration of the call.
    let status = unsafe { bind(socket.raw(), address.raw(), LocalSocketAddress::byte_len()?) };
    ensure!(
        status != SOCKET_ERROR,
        "bind AF_UNIX: {}",
        last_winsock_error()
    );
    Ok(())
}

fn listen_socket(socket: &OwnedSocket) -> Result<()> {
    // SAFETY: socket is bound and the backlog is positive.
    let status = unsafe { listen(socket.raw(), 1) };
    ensure!(
        status != SOCKET_ERROR,
        "listen AF_UNIX: {}",
        last_winsock_error()
    );
    Ok(())
}

fn connect_socket(socket: &OwnedSocket, address: &LocalSocketAddress) -> Result<()> {
    // SAFETY: address points to a fully initialized SOCKADDR_UN for the duration of the call.
    let status = unsafe { connect(socket.raw(), address.raw(), LocalSocketAddress::byte_len()?) };
    ensure!(
        status != SOCKET_ERROR,
        "connect AF_UNIX: {}",
        last_winsock_error()
    );
    Ok(())
}

fn accept_with_deadline(listener: &OwnedSocket, timeout: Duration) -> Result<OwnedSocket> {
    let event = OwnedWsaEvent::create()?;
    // SAFETY: listener and event are valid; FD_ACCEPT requests one network-event class.
    let status = unsafe { WSAEventSelect(listener.raw(), event.0, FD_ACCEPT.cast_signed()) };
    ensure!(
        status != SOCKET_ERROR,
        "select AF_UNIX accept event: {}",
        last_winsock_error()
    );
    // SAFETY: event is a live kernel event and timeout is policy-bounded.
    let wait = unsafe { WaitForSingleObject(event.handle(), u32::try_from(timeout.as_millis())?) };
    if wait == WAIT_TIMEOUT {
        bail!("timed out accepting AF_UNIX child");
    }
    if wait != WAIT_OBJECT_0 {
        return Err(std::io::Error::last_os_error()).context("wait for AF_UNIX child connection");
    }
    let mut events = WSANETWORKEVENTS::default();
    // SAFETY: listener/event are paired and events is writable.
    let status = unsafe { WSAEnumNetworkEvents(listener.raw(), event.0, &raw mut events) };
    ensure!(
        status != SOCKET_ERROR,
        "read AF_UNIX accept event: {}",
        last_winsock_error()
    );
    ensure!(
        events.lNetworkEvents & FD_ACCEPT.cast_signed() != 0,
        "AF_UNIX event omitted FD_ACCEPT"
    );
    let accept_error = events.iErrorCode[FD_ACCEPT_BIT as usize];
    ensure!(accept_error == 0, "AF_UNIX accept error {accept_error}");
    // SAFETY: the event reports a pending connection; address outputs are optional.
    let socket = unsafe { accept(listener.raw(), null_mut(), null_mut()) };
    ensure!(
        socket != INVALID_SOCKET,
        "accept AF_UNIX: {}",
        last_winsock_error()
    );
    Ok(OwnedSocket(socket))
}

fn set_socket_deadlines(socket: &OwnedSocket, timeout: Duration) -> Result<()> {
    let milliseconds = u32::try_from(timeout.as_millis())?;
    for option in [SO_RCVTIMEO, SO_SNDTIMEO] {
        // SAFETY: the option value is a live u32 and the socket is valid.
        let status = unsafe {
            setsockopt(
                socket.raw(),
                SOL_SOCKET,
                option,
                std::ptr::from_ref(&milliseconds).cast(),
                i32::try_from(size_of::<u32>())?,
            )
        };
        ensure!(
            status != SOCKET_ERROR,
            "set AF_UNIX socket deadline: {}",
            last_winsock_error()
        );
    }
    Ok(())
}

fn set_blocking(socket: &OwnedSocket) -> Result<()> {
    // SAFETY: socket is valid; a null event and zero mask detach inherited event selection.
    let status = unsafe { WSAEventSelect(socket.raw(), WSA_INVALID_EVENT, 0) };
    ensure!(
        status != SOCKET_ERROR,
        "detach AF_UNIX socket event selection: {}",
        last_winsock_error()
    );
    let mut nonblocking = 0_u32;
    // SAFETY: socket is valid and nonblocking is a writable mode value.
    let status = unsafe { ioctlsocket(socket.raw(), FIONBIO, &raw mut nonblocking) };
    ensure!(
        status != SOCKET_ERROR,
        "restore blocking AF_UNIX socket: {}",
        last_winsock_error()
    );
    Ok(())
}

fn send_all(socket: &OwnedSocket, mut bytes: &[u8]) -> Result<()> {
    while !bytes.is_empty() {
        // SAFETY: bytes points to readable memory for the supplied length and socket is valid.
        let sent = unsafe { send(socket.raw(), bytes.as_ptr(), i32::try_from(bytes.len())?, 0) };
        if sent == SOCKET_ERROR {
            bail!("send AF_UNIX: {}", last_winsock_error());
        }
        ensure!(sent > 0, "AF_UNIX peer disconnected during send");
        bytes = &bytes[usize::try_from(sent)?..];
    }
    Ok(())
}

fn receive_exact(socket: &OwnedSocket, mut bytes: &mut [u8]) -> Result<()> {
    while !bytes.is_empty() {
        // SAFETY: bytes points to writable memory for the supplied length and socket is valid.
        let received = unsafe {
            recv(
                socket.raw(),
                bytes.as_mut_ptr(),
                i32::try_from(bytes.len())?,
                0,
            )
        };
        if received == SOCKET_ERROR {
            bail!("receive AF_UNIX: {}", last_winsock_error());
        }
        ensure!(received > 0, "AF_UNIX peer disconnected during receive");
        let (_, remaining) = bytes.split_at_mut(usize::try_from(received)?);
        bytes = remaining;
    }
    Ok(())
}

fn last_winsock_error() -> i32 {
    // SAFETY: WSAGetLastError has no preconditions.
    unsafe { WSAGetLastError() }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::path::PathBuf;

    use super::LocalSocketAddress;

    #[test]
    fn af_unix_rejects_wtf16_without_lossy_conversion() {
        let path = PathBuf::from(OsString::from_wide(&[
            b'C'.into(),
            b':'.into(),
            b'\\'.into(),
            0xd800,
            b'.'.into(),
            b's'.into(),
            b'o'.into(),
            b'c'.into(),
            b'k'.into(),
        ]));

        let Err(error) = LocalSocketAddress::new(&path) else {
            panic!("WTF-16 must not become UTF-8");
        };
        assert!(
            format!("{error:#}").contains("cannot represent a WTF-16 endpoint"),
            "unexpected error: {error:#}"
        );
    }
}
