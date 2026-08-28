use std::fs::{File, OpenOptions};
use std::io;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use windows_sys::Win32::Networking::WinSock::{WSACleanup, WSADATA, WSAStartup};

use crate::protocol::{
    ChildFrame, ExpectedOutcome, HostFrame, ObservedOutcome, ProbeOutcome, RuntimeKind, read_frame,
    write_frame,
};
use crate::windows::{
    clipboard_probe, current_child_facts, harden_dll_search, parent_process_probe, probe_allowed,
    probe_denied, registry_probe,
};

/// Runs the restricted extension child's authenticated probe sequence.
///
/// # Errors
///
/// Returns an error when bootstrap data is invalid, IPC fails, or a broker response violates the
/// protocol.
pub fn run(runtime: RuntimeKind) -> Result<()> {
    let dll_search_hardened = harden_dll_search();
    let pipe = required_env("KOMOREBI_PROTOTYPE_PIPE")?;
    let nonce = required_env("KOMOREBI_PROTOTYPE_NONCE")?;
    let package_file = required_env("KOMOREBI_PROTOTYPE_PACKAGE_FILE")?;
    let denied_file = required_env("KOMOREBI_PROTOTYPE_DENIED_FILE")?;
    let parent_pid = required_env("KOMOREBI_PROTOTYPE_PARENT_PID")?.parse::<u32>()?;

    let facts = current_child_facts(dll_search_hardened)?;
    let mut pipe = open_pipe(&pipe).context("open authenticated host pipe")?;
    write_frame(
        &mut pipe,
        &ChildFrame::Hello {
            nonce,
            runtime,
            facts,
        },
    )?;
    let welcome: HostFrame = read_frame(&mut pipe)?;
    let HostFrame::Welcome { generation: _ } = welcome else {
        bail!("host did not send welcome");
    };

    let mut probes = vec![
        file_probe("package_read", &package_file, ExpectedOutcome::Allowed),
        file_probe("host_private_file", &denied_file, ExpectedOutcome::Denied),
        file_probe(
            "user_profile_file",
            format!(
                r"C:\Users\{}\NTUSER.DAT",
                std::env::var("USERNAME").unwrap_or_default()
            ),
            ExpectedOutcome::Denied,
        ),
        file_probe(
            "system_sam",
            r"C:\Windows\System32\config\SAM",
            ExpectedOutcome::Denied,
        ),
        registry_probe(),
        parent_process_probe(parent_pid),
        clipboard_probe(),
        child_process_probe(),
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
    ];
    probes.extend(network_probes());
    probes.sort_unstable_by(|left, right| left.name.cmp(&right.name));
    write_frame(&mut pipe, &ChildFrame::ProbeReport { probes })?;

    let mut echo_rtt_us = Vec::new();
    for sequence in 0..32_u64 {
        let started = Instant::now();
        let sent_ticks = u64::try_from(Instant::now().elapsed().as_nanos()).unwrap_or(u64::MAX);
        write_frame(
            &mut pipe,
            &ChildFrame::Echo {
                sequence,
                sent_ticks,
            },
        )?;
        let echoed: HostFrame = read_frame(&mut pipe)?;
        if !matches!(echoed, HostFrame::Echoed { sequence: seen, .. } if seen == sequence) {
            bail!("echo sequence mismatch");
        }
        echo_rtt_us.push(started.elapsed().as_secs_f64() * 1_000_000.0);
    }

    write_frame(
        &mut pipe,
        &ChildFrame::StoragePut {
            request: 1,
            key: "roundtrip".to_owned(),
            expected_revision: 0,
            value: b"private-value".to_vec(),
        },
    )?;
    let _: HostFrame = read_frame(&mut pipe)?;
    write_frame(
        &mut pipe,
        &ChildFrame::StorageGet {
            request: 2,
            key: "roundtrip".to_owned(),
        },
    )?;
    let _: HostFrame = read_frame(&mut pipe)?;
    write_frame(
        &mut pipe,
        &ChildFrame::HttpGet {
            request: 3,
            url: "http://example.com/".to_owned(),
        },
    )?;
    let _: HostFrame = read_frame(&mut pipe)?;
    write_frame(&mut pipe, &ChildFrame::Goodbye { echo_rtt_us })?;
    Ok(())
}

fn required_env(key: &str) -> Result<String> {
    std::env::var(key).with_context(|| format!("missing {key}"))
}

fn open_pipe(path: &str) -> io::Result<File> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match OpenOptions::new().read(true).write(true).open(path) {
            Ok(file) => return Ok(file),
            Err(error) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
                let _ = error;
            }
            Err(error) => return Err(error),
        }
    }
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
    let command =
        Path::new(&std::env::var("SYSTEMROOT").unwrap_or_else(|_| r"C:\Windows".to_owned()))
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
