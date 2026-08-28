use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use anyhow::{Context, Result, bail, ensure};
use windows_sys::Win32::Foundation::HANDLE;

use crate::protocol::{ChildFrame, ExtensionGeneration, HostFrame, ProbeOutcome};

use super::broker_http;
use super::ipc::PipeChannel;
use super::policy::ContainmentPolicy;
use super::report::Verification;

pub(super) struct SessionEvidence {
    pub(super) probes: Vec<ProbeOutcome>,
    pub(super) echo_rtt_us: Vec<f64>,
    pub(super) broker_service_us: Vec<f64>,
    pub(super) storage_cas_roundtrip: Verification,
    pub(super) brokered_http_status: Option<u16>,
    pub(super) stale_generation_rejected: Verification,
}

struct SessionState {
    storage: HashMap<String, (u64, Vec<u8>)>,
    probes: Vec<ProbeOutcome>,
    broker_service_us: Vec<f64>,
    storage_cas_roundtrip: bool,
    brokered_http_status: Option<u16>,
    stale_generation_rejected: bool,
}

enum SessionControl {
    Continue,
    Finish(Vec<f64>),
}

pub(super) fn serve(
    channel: &mut PipeChannel,
    generation: ExtensionGeneration,
    process: HANDLE,
    error_file: &Path,
    policy: &ContainmentPolicy,
) -> Result<SessionEvidence> {
    let mut state = SessionState::new();
    let echo_rtt_us = loop {
        let frame = channel
            .receive(policy.pipe().operation_timeout())
            .with_context(|| super::child_error_detail(process, error_file))?;
        if frame.generation() != Some(generation) {
            state.reject_stale(channel, &frame)?;
            continue;
        }
        let service_started = Instant::now();
        let control = state.handle(channel, frame, policy)?;
        state
            .broker_service_us
            .push(service_started.elapsed().as_secs_f64() * 1_000_000.0);
        if let SessionControl::Finish(echo_rtt_us) = control {
            break echo_rtt_us;
        }
    };
    ensure!(
        state.stale_generation_rejected,
        "child did not exercise stale-generation rejection"
    );
    Ok(SessionEvidence {
        probes: state.probes,
        echo_rtt_us,
        broker_service_us: state.broker_service_us,
        storage_cas_roundtrip: Verification::from(state.storage_cas_roundtrip),
        brokered_http_status: state.brokered_http_status,
        stale_generation_rejected: Verification::from(state.stale_generation_rejected),
    })
}

impl SessionState {
    fn new() -> Self {
        Self {
            storage: HashMap::new(),
            probes: Vec::new(),
            broker_service_us: Vec::new(),
            storage_cas_roundtrip: false,
            brokered_http_status: None,
            stale_generation_rejected: false,
        }
    }

    fn reject_stale(&mut self, channel: &mut PipeChannel, frame: &ChildFrame) -> Result<()> {
        ensure!(
            frame.generation().is_some(),
            "duplicate hello after authentication"
        );
        channel.send(&HostFrame::Rejected {
            request: frame.request_id(),
            code: "stale_generation".to_owned(),
        })?;
        self.stale_generation_rejected = true;
        Ok(())
    }

    fn handle(
        &mut self,
        channel: &mut PipeChannel,
        frame: ChildFrame,
        policy: &ContainmentPolicy,
    ) -> Result<SessionControl> {
        match frame {
            ChildFrame::Echo {
                generation: _,
                sequence,
                sent_ticks,
            } => channel.send(&HostFrame::Echoed {
                sequence,
                sent_ticks,
            })?,
            ChildFrame::StoragePut {
                generation: _,
                request,
                key,
                expected_revision,
                value,
            } => self.storage_put(
                channel,
                request,
                key,
                expected_revision,
                value,
                policy.workload().storage_value_limit_bytes(),
            )?,
            ChildFrame::StorageGet {
                generation: _,
                request,
                key,
            } => self.storage_get(channel, request, &key)?,
            ChildFrame::HttpGet {
                generation: _,
                request,
                url,
            } => self.http_get(channel, request, &url)?,
            ChildFrame::ProbeReport {
                generation: _,
                probes,
            } => self.probes = probes,
            ChildFrame::Goodbye {
                generation: _,
                echo_rtt_us,
            } => return Ok(SessionControl::Finish(echo_rtt_us)),
            ChildFrame::Hello { .. } => bail!("duplicate hello"),
            ChildFrame::FaultArmed { .. } => bail!("unexpected fault frame in normal session"),
        }
        Ok(SessionControl::Continue)
    }

    fn storage_put(
        &mut self,
        channel: &mut PipeChannel,
        request: u64,
        key: String,
        expected_revision: u64,
        value: Vec<u8>,
        value_limit: usize,
    ) -> Result<()> {
        let current = self.storage.get(&key).map_or(0, |(revision, _)| *revision);
        if current == expected_revision && value.len() <= value_limit {
            let revision = current + 1;
            self.storage.insert(key, (revision, value));
            self.storage_cas_roundtrip = true;
            channel.send(&HostFrame::StorageStored { request, revision })
        } else {
            channel.send(&HostFrame::Rejected {
                request: Some(request),
                code: "storage_conflict_or_limit".to_owned(),
            })
        }
    }

    fn storage_get(&self, channel: &mut PipeChannel, request: u64, key: &str) -> Result<()> {
        if let Some((revision, value)) = self.storage.get(key) {
            channel.send(&HostFrame::StorageValue {
                request,
                revision: *revision,
                value: value.clone(),
            })
        } else {
            channel.send(&HostFrame::Rejected {
                request: Some(request),
                code: "storage_missing".to_owned(),
            })
        }
    }

    fn http_get(&mut self, channel: &mut PipeChannel, request: u64, url: &str) -> Result<()> {
        match broker_http(url) {
            Ok((status, bytes)) => {
                self.brokered_http_status = Some(status);
                channel.send(&HostFrame::HttpResult {
                    request,
                    status,
                    bytes,
                })
            }
            Err(error) => channel.send(&HostFrame::Rejected {
                request: Some(request),
                code: format!("http_policy_or_transport:{error}"),
            }),
        }
    }
}
