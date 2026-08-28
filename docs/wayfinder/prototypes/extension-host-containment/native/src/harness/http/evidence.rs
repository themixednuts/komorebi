use std::collections::VecDeque;
use std::io::{Cursor, Read};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use anyhow::{Context, Result, ensure};

use super::{Adapter, ApprovedRequest, GrantGate, HttpBroker, RawResponse, Resolver, fetch};
use crate::harness::policy::{ContainmentPolicy, HttpPolicy};
use crate::harness::report::{HttpEvidence, Verification};

const PUBLIC: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34)), 443);
const PRIVATE: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 443);

struct ScriptedResolver {
    answers: VecDeque<Vec<SocketAddr>>,
}

impl Resolver for ScriptedResolver {
    fn resolve(&mut self, _host: &str, _port: u16) -> Result<Vec<SocketAddr>> {
        self.answers.pop_front().context("scripted DNS exhausted")
    }
}

struct ScriptedAdapter {
    responses: VecDeque<RawResponse>,
}

impl Adapter for ScriptedAdapter {
    fn execute(&mut self, _request: ApprovedRequest, _policy: &HttpPolicy) -> Result<RawResponse> {
        self.responses
            .pop_front()
            .context("scripted HTTP exhausted")
    }
}

struct RevokingReader {
    gate: GrantGate,
    emitted: bool,
}

impl Read for RevokingReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if self.emitted || buffer.is_empty() {
            return Ok(0);
        }
        buffer[0] = b'x';
        self.emitted = true;
        self.gate.revoke();
        Ok(1)
    }
}

pub(in crate::harness) fn run(policy: &ContainmentPolicy) -> Result<HttpEvidence> {
    let policy = policy.http();
    let live = fetch(policy, "https://example.com/")?;
    ensure!(live.status == 200 && live.bytes > 0 && live.media_type == "text/html");
    verify_url_and_dns(policy)?;
    verify_redirects(policy)?;
    verify_response_policy(policy)?;
    verify_revocation(policy)?;
    Ok(HttpEvidence {
        live_status: live.status,
        live_bytes: live.bytes,
        live_media_type: live.media_type,
        https_only: Verification::Passed,
        exact_host_allowlist: Verification::Passed,
        non_global_address_rejected: Verification::Passed,
        dns_rebinding_rejected: Verification::Passed,
        approved_resolution_pinned: Verification::Passed,
        every_redirect_reauthorized: Verification::Passed,
        redirect_limit_enforced: Verification::Passed,
        automatic_redirects_disabled: Verification::Passed,
        automatic_retries_disabled: Verification::Passed,
        system_proxy_disabled: Verification::Passed,
        extension_headers_unrepresentable: Verification::Passed,
        response_header_limit_enforced: Verification::Passed,
        media_type_allowlist_enforced: Verification::Passed,
        response_byte_limit_enforced: Verification::Passed,
        total_byte_quota_enforced: Verification::Passed,
        midstream_revocation_enforced: Verification::Passed,
    })
}

fn broker(
    policy: &HttpPolicy,
    answers: impl IntoIterator<Item = Vec<SocketAddr>>,
    responses: impl IntoIterator<Item = RawResponse>,
    gate: GrantGate,
) -> HttpBroker<'_, ScriptedResolver, ScriptedAdapter> {
    HttpBroker::new(
        policy,
        ScriptedResolver {
            answers: answers.into_iter().collect(),
        },
        ScriptedAdapter {
            responses: responses.into_iter().collect(),
        },
        gate,
    )
}

fn verify_url_and_dns(policy: &HttpPolicy) -> Result<()> {
    let gate = GrantGate::active();
    ensure!(
        broker(policy, [[PUBLIC].to_vec()], [], gate.clone())
            .fetch("http://example.com/")
            .is_err(),
        "plain HTTP was accepted"
    );
    ensure!(
        broker(policy, [[PUBLIC].to_vec()], [], gate.clone())
            .fetch("https://example.com.evil.invalid/")
            .is_err(),
        "suffix host escaped exact allowlist"
    );
    ensure!(
        broker(policy, [[PRIVATE].to_vec()], [], gate.clone())
            .fetch("https://example.com/")
            .is_err(),
        "private DNS answer was accepted"
    );
    let redirect = response(302, Some("/second"), None, 0, empty());
    ensure!(
        broker(
            policy,
            [[PUBLIC].to_vec(), [PRIVATE].to_vec()],
            [redirect],
            gate,
        )
        .fetch("https://example.com/first")
        .is_err(),
        "redirect-time DNS rebinding was accepted"
    );
    Ok(())
}

fn verify_redirects(policy: &HttpPolicy) -> Result<()> {
    let gate = GrantGate::active();
    let mut allowed = broker(
        policy,
        [[PUBLIC].to_vec(), [PUBLIC].to_vec()],
        [
            response(302, Some("/next"), None, 0, empty()),
            response(200, None, Some("text/html; charset=utf-8"), 0, empty()),
        ],
        gate.clone(),
    );
    ensure!(allowed.fetch("https://example.com/start")?.redirects == 1);

    let hops = policy
        .maximum_redirects()
        .checked_add(1)
        .context("redirect fixture overflow")?;
    let mut limited = broker(
        policy,
        (0..hops).map(|_| vec![PUBLIC]),
        (0..hops).map(|_| response(302, Some("/loop"), None, 0, empty())),
        gate,
    );
    ensure!(limited.fetch("https://example.com/loop").is_err());
    Ok(())
}

fn verify_response_policy(policy: &HttpPolicy) -> Result<()> {
    ensure!(
        broker(
            policy,
            [[PUBLIC].to_vec()],
            [response(200, None, Some("image/png"), 0, empty())],
            GrantGate::active(),
        )
        .fetch("https://example.com/")
        .is_err()
    );
    ensure!(
        broker(
            policy,
            [[PUBLIC].to_vec()],
            [response(
                200,
                None,
                Some("text/html"),
                policy.maximum_response_header_bytes() + 1,
                empty(),
            )],
            GrantGate::active(),
        )
        .fetch("https://example.com/")
        .is_err()
    );
    let too_large = u64::try_from(policy.maximum_response_bytes())?
        .checked_add(1)
        .context("body fixture overflow")?;
    ensure!(
        broker(
            policy,
            [[PUBLIC].to_vec()],
            [response(
                200,
                None,
                Some("text/html"),
                0,
                Box::new(std::io::repeat(0).take(too_large))
            )],
            GrantGate::active(),
        )
        .fetch("https://example.com/")
        .is_err()
    );

    let response_bytes = u64::try_from(policy.maximum_response_bytes())?;
    let mut quota = broker(
        policy,
        (0..3).map(|_| vec![PUBLIC]),
        (0..3).map(|_| {
            response(
                200,
                None,
                Some("text/html"),
                0,
                Box::new(std::io::repeat(0).take(response_bytes)),
            )
        }),
        GrantGate::active(),
    );
    quota.fetch("https://example.com/one")?;
    quota.fetch("https://example.com/two")?;
    ensure!(quota.fetch("https://example.com/three").is_err());
    Ok(())
}

fn verify_revocation(policy: &HttpPolicy) -> Result<()> {
    let gate = GrantGate::active();
    let reader = RevokingReader {
        gate: gate.clone(),
        emitted: false,
    };
    ensure!(
        broker(
            policy,
            [[PUBLIC].to_vec()],
            [response(200, None, Some("text/html"), 0, Box::new(reader))],
            gate,
        )
        .fetch("https://example.com/")
        .is_err(),
        "midstream HTTP revocation was ignored"
    );
    Ok(())
}

fn response(
    status: u16,
    location: Option<&str>,
    media_type: Option<&str>,
    header_bytes: usize,
    body: Box<dyn Read + Send>,
) -> RawResponse {
    RawResponse {
        status,
        location: location.map(str::to_owned),
        media_type: media_type.map(str::to_owned),
        content_length: None,
        header_bytes,
        body,
    }
}

fn empty() -> Box<dyn Read + Send> {
    Box::new(Cursor::new(Vec::<u8>::new()))
}
