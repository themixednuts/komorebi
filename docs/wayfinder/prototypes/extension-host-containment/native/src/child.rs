use std::ffi::OsStr;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use uuid::Uuid;
use windows_sys::Win32::Networking::WinSock::{WSACleanup, WSADATA, WSAStartup};

use crate::child_pipe;
use crate::protocol::{
    ChildFrame, ExpectedOutcome, ExtensionGeneration, FrameCodec, FrameLimit, HostFrame,
    ObservedOutcome, ProbeOutcome, RuntimeKind,
};
use crate::windows::{
    clipboard_probe, current_child_facts, harden_dll_search, other_window_message_probe,
    parent_process_duplicate_handle_probe, parent_process_injection_probe, parent_process_probe,
    probe_allowed, probe_denied, registry_probe,
};

/// Runs the restricted extension child's authenticated probe sequence.
///
/// # Errors
///
/// Returns an error when bootstrap data is invalid, IPC fails, or a broker response violates the
/// protocol.
pub fn run(runtime: RuntimeKind) -> Result<()> {
    trace("child:start");
    let dll_search_hardened = harden_dll_search();
    let pipe = required_text_env("KOMOREBI_PROTOTYPE_PIPE")?;
    let nonce = required_text_env("KOMOREBI_PROTOTYPE_NONCE")?.parse::<Uuid>()?;
    let package_file = required_path_env("KOMOREBI_PROTOTYPE_PACKAGE_FILE")?;
    let denied_file = required_path_env("KOMOREBI_PROTOTYPE_DENIED_FILE")?;
    let denied_verbatim_file = required_path_env("KOMOREBI_PROTOTYPE_DENIED_VERBATIM_FILE")?;
    let foreign_file = required_path_env("KOMOREBI_PROTOTYPE_FOREIGN_FILE")?;
    let reparse_file = required_path_env("KOMOREBI_PROTOTYPE_REPARSE_FILE")?;
    let parent_pid = required_text_env("KOMOREBI_PROTOTYPE_PARENT_PID")?.parse::<u32>()?;
    let frame_limit =
        FrameLimit::new(required_text_env("KOMOREBI_PROTOTYPE_FRAME_LIMIT")?.parse::<usize>()?)?;
    let pipe_timeout = Duration::from_millis(u64::from(
        required_text_env("KOMOREBI_PROTOTYPE_PIPE_TIMEOUT_MS")?.parse::<u32>()?,
    ));
    let echo_samples = required_text_env("KOMOREBI_PROTOTYPE_ECHO_SAMPLES")?.parse::<u64>()?;
    let codec = FrameCodec::new(frame_limit);

    let facts = current_child_facts(dll_search_hardened)?;
    trace("child:wait_pipe");
    let mut pipe = child_pipe::open(&pipe, pipe_timeout).context("open authenticated host pipe")?;
    trace("child:pipe_open");
    codec.write(
        &mut pipe,
        &ChildFrame::Hello {
            nonce,
            runtime,
            facts,
        },
    )?;
    trace("child:hello_sent");
    let welcome: HostFrame = codec.read(&mut pipe)?;
    trace("child:welcome_received");
    let HostFrame::Welcome { generation } = welcome else {
        bail!("host did not send welcome");
    };

    let inputs = SessionInputs {
        package_file: &package_file,
        denied_file: &denied_file,
        denied_verbatim_file: &denied_verbatim_file,
        foreign_file: &foreign_file,
        reparse_file: &reparse_file,
        parent_pid,
        echo_samples,
    };
    exercise_authenticated_session(&mut pipe, codec, generation, inputs)
}

#[derive(Clone, Copy)]
struct SessionInputs<'a> {
    package_file: &'a Path,
    denied_file: &'a Path,
    denied_verbatim_file: &'a Path,
    foreign_file: &'a Path,
    reparse_file: &'a Path,
    parent_pid: u32,
    echo_samples: u64,
}

fn exercise_authenticated_session(
    pipe: &mut File,
    codec: FrameCodec,
    generation: ExtensionGeneration,
    inputs: SessionInputs<'_>,
) -> Result<()> {
    if let Some(stale_generation) = generation.previous() {
        codec.write(
            &mut *pipe,
            &ChildFrame::Echo {
                generation: stale_generation,
                sequence: u64::MAX,
                sent_ticks: 0,
            },
        )?;
        let rejection: HostFrame = codec.read(&mut *pipe)?;
        ensure!(
            matches!(
                rejection,
                HostFrame::Rejected { request: None, ref code }
                    if code == "stale_generation"
            ),
            "host accepted a stale generation"
        );
    }

    let probes = boundary_probes(
        inputs.package_file,
        inputs.denied_file,
        inputs.denied_verbatim_file,
        inputs.foreign_file,
        inputs.reparse_file,
        inputs.parent_pid,
    );
    codec.write(&mut *pipe, &ChildFrame::ProbeReport { generation, probes })?;
    trace("child:probes_sent");

    let mut echo_rtt_us = Vec::new();
    for sequence in 0..inputs.echo_samples {
        let started = Instant::now();
        let sent_ticks = u64::try_from(Instant::now().elapsed().as_nanos()).unwrap_or(u64::MAX);
        codec.write(
            &mut *pipe,
            &ChildFrame::Echo {
                generation,
                sequence,
                sent_ticks,
            },
        )?;
        let echoed: HostFrame = codec.read(&mut *pipe)?;
        if !matches!(echoed, HostFrame::Echoed { sequence: seen, .. } if seen == sequence) {
            bail!("echo sequence mismatch");
        }
        echo_rtt_us.push(started.elapsed().as_secs_f64() * 1_000_000.0);
    }

    codec.write(
        &mut *pipe,
        &ChildFrame::StoragePut {
            generation,
            request: 1,
            key: "roundtrip".to_owned(),
            expected_revision: 0,
            value: b"private-value".to_vec(),
        },
    )?;
    let _: HostFrame = codec.read(&mut *pipe)?;
    codec.write(
        &mut *pipe,
        &ChildFrame::StorageGet {
            generation,
            request: 2,
            key: "roundtrip".to_owned(),
        },
    )?;
    let _: HostFrame = codec.read(&mut *pipe)?;
    codec.write(
        &mut *pipe,
        &ChildFrame::HttpGet {
            generation,
            request: 3,
            url: "http://example.com/".to_owned(),
        },
    )?;
    let _: HostFrame = codec.read(&mut *pipe)?;
    codec.write(
        &mut *pipe,
        &ChildFrame::Goodbye {
            generation,
            echo_rtt_us,
        },
    )?;
    trace("child:goodbye_sent");
    Ok(())
}

fn trace(stage: &str) {
    if std::env::var_os("KOMOREBI_PROTOTYPE_TRACE").as_deref() != Some(OsStr::new("1")) {
        return;
    }
    let Some(path) = std::env::var_os("KOMOREBI_PROTOTYPE_ERROR_FILE") else {
        return;
    };
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        // This trace is diagnostic-only; the authenticated pipe remains the authoritative result.
        let _ = writeln!(file, "{stage}");
    }
}

fn boundary_probes(
    package_file: &Path,
    denied_file: &Path,
    denied_verbatim_file: &Path,
    foreign_file: &Path,
    reparse_file: &Path,
    parent_pid: u32,
) -> Vec<ProbeOutcome> {
    let mut probes = vec![
        traced_probe("probe:package_read", || {
            file_probe("package_wtf16_read", package_file, ExpectedOutcome::Allowed)
        }),
        traced_probe("probe:host_private_file", || {
            file_probe("host_private_file", denied_file, ExpectedOutcome::Denied)
        }),
        file_probe(
            "extended_dos_private_file",
            denied_verbatim_file,
            ExpectedOutcome::Denied,
        ),
        file_probe(
            "cross_extension_private_file",
            foreign_file,
            ExpectedOutcome::Denied,
        ),
        file_probe("reparse_escape", reparse_file, ExpectedOutcome::Denied),
        file_probe(
            "user_profile_file",
            user_profile_file(),
            ExpectedOutcome::Denied,
        ),
        file_probe(
            "system_sam",
            r"C:\Windows\System32\config\SAM",
            ExpectedOutcome::Denied,
        ),
        traced_probe("probe:registry", registry_probe),
        traced_probe("probe:parent_read", || parent_process_probe(parent_pid)),
        traced_probe("probe:parent_injection", || {
            parent_process_injection_probe(parent_pid)
        }),
        traced_probe("probe:parent_duplicate", || {
            parent_process_duplicate_handle_probe(parent_pid)
        }),
        traced_probe("probe:clipboard", clipboard_probe),
        traced_probe("probe:window_message", other_window_message_probe),
        traced_probe("probe:child_process", child_process_probe),
        file_probe(
            "device_namespace",
            r"\\.\PhysicalDrive0",
            ExpectedOutcome::Denied,
        ),
        file_probe(
            "unc_path",
            r"\\localhost\c$\Windows\win.ini",
            ExpectedOutcome::Denied,
        ),
        file_probe(
            "extended_device_path",
            r"\\?\GLOBALROOT\Device\HarddiskVolumeShadowCopy1\Windows\win.ini",
            ExpectedOutcome::Denied,
        ),
        file_probe(
            "extended_dos_public_file",
            r"\\?\C:\Windows\win.ini",
            ExpectedOutcome::Allowed,
        ),
    ];
    probes.extend(network_probes());
    probes.sort_unstable_by(|left, right| left.name.cmp(&right.name));
    probes
}

fn traced_probe(name: &'static str, probe: impl FnOnce() -> ProbeOutcome) -> ProbeOutcome {
    trace(name);
    probe()
}

fn required_text_env(key: &str) -> Result<String> {
    std::env::var(key).with_context(|| format!("missing {key}"))
}

fn required_path_env(key: &str) -> Result<PathBuf> {
    std::env::var_os(key)
        .map(PathBuf::from)
        .with_context(|| format!("missing {key}"))
}

fn user_profile_file() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map_or_else(|| PathBuf::from(r"C:\Users\Default"), PathBuf::from)
        .join("NTUSER.DAT")
}

fn file_probe(name: &str, path: impl AsRef<Path>, expected: ExpectedOutcome) -> ProbeOutcome {
    match File::open(path) {
        Ok(_) => probe_allowed(name, expected),
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            probe_denied(name, expected, error.raw_os_error())
        }
        Err(error) => ProbeOutcome {
            name: name.to_owned(),
            expected,
            observed: ObservedOutcome::Unavailable {
                reason: error.to_string(),
            },
        },
    }
}

fn tcp_probe(name: &str, address: &str) -> ProbeOutcome {
    let parsed = address.parse::<SocketAddr>();
    let observed = match parsed {
        Ok(address) => match TcpStream::connect_timeout(&address, Duration::from_millis(250)) {
            Ok(_) => ObservedOutcome::Allowed,
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                ObservedOutcome::Denied {
                    os_error: error.raw_os_error(),
                }
            }
            Err(error) => ObservedOutcome::Unavailable {
                reason: error.to_string(),
            },
        },
        Err(error) => ObservedOutcome::Unavailable {
            reason: error.to_string(),
        },
    };
    ProbeOutcome {
        name: name.to_owned(),
        expected: ExpectedOutcome::Denied,
        observed,
    }
}

fn network_probes() -> Vec<ProbeOutcome> {
    let mut data = WSADATA::default();
    // SAFETY: data is writable and version 2.2 is the documented Winsock request.
    let status = unsafe { WSAStartup(0x0202, &raw mut data) };
    if status != 0 {
        return ["direct_loopback", "direct_ipv6", "direct_dns"]
            .into_iter()
            .map(|name| probe_denied(name, ExpectedOutcome::Denied, Some(status)))
            .collect();
    }
    let results = vec![
        tcp_probe("direct_loopback", "127.0.0.1:9"),
        tcp_probe("direct_ipv6", "[::1]:9"),
        dns_probe(),
    ];
    // SAFETY: this balances the successful WSAStartup in this function.
    unsafe { WSACleanup() };
    results
}

fn dns_probe() -> ProbeOutcome {
    let observed = match ("example.com", 443).to_socket_addrs() {
        Ok(addresses) => {
            if addresses.into_iter().next().is_some() {
                ObservedOutcome::Allowed
            } else {
                ObservedOutcome::Unavailable {
                    reason: "resolver returned no addresses".to_owned(),
                }
            }
        }
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => ObservedOutcome::Denied {
            os_error: error.raw_os_error(),
        },
        Err(error) => ObservedOutcome::Unavailable {
            reason: error.to_string(),
        },
    };
    ProbeOutcome {
        name: "direct_dns".to_owned(),
        expected: ExpectedOutcome::Denied,
        observed,
    }
}

fn child_process_probe() -> ProbeOutcome {
    let command = std::env::var_os("SYSTEMROOT")
        .map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from)
        .join("System32")
        .join("cmd.exe");
    let observed = match Command::new(command)
        .args(["/d", "/c", "exit", "0"])
        .status()
    {
        Ok(_) => ObservedOutcome::Allowed,
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => ObservedOutcome::Denied {
            os_error: error.raw_os_error(),
        },
        Err(error) => ObservedOutcome::Unavailable {
            reason: error.to_string(),
        },
    };
    ProbeOutcome {
        name: "child_process_creation".to_owned(),
        expected: ExpectedOutcome::Denied,
        observed,
    }
}
