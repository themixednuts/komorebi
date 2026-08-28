mod adapter;
mod evidence;

use std::io::Read;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::num::NonZeroU64;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result, ensure};
use http_acl::{HttpAcl, HttpAclBuilder};
use mime::Mime;
use url::Url;

use self::adapter::ReqwestAdapter;
use super::policy::HttpPolicy;
pub(super) use evidence::run as run_evidence;

pub(super) trait Resolver {
    fn resolve(&mut self, host: &str, port: u16) -> Result<Vec<SocketAddr>>;
}

pub(super) trait Adapter {
    fn execute(&mut self, request: ApprovedRequest, policy: &HttpPolicy) -> Result<RawResponse>;
}

pub(super) struct SystemResolver;

impl Resolver for SystemResolver {
    fn resolve(&mut self, host: &str, port: u16) -> Result<Vec<SocketAddr>> {
        (host, port)
            .to_socket_addrs()
            .context("resolve brokered HTTP target")
            .map(Iterator::collect)
    }
}

pub(super) struct ApprovedRequest {
    url: Url,
    host: String,
    addresses: Box<[SocketAddr]>,
}

pub(super) struct RawResponse {
    status: u16,
    location: Option<String>,
    media_type: Option<String>,
    content_length: Option<u64>,
    header_bytes: usize,
    body: Box<dyn Read + Send>,
}

pub(super) struct BrokerResponse {
    pub(super) status: u16,
    pub(super) media_type: String,
    pub(super) body: Vec<u8>,
    pub(super) redirects: usize,
}

pub(super) struct HttpFetchResponse {
    pub(super) status: u16,
    pub(super) media_type: String,
    pub(super) bytes: usize,
}

pub(super) fn fetch(policy: &HttpPolicy, input: &str) -> Result<HttpFetchResponse> {
    let mut broker = HttpBroker::new(policy, SystemResolver, ReqwestAdapter, GrantGate::active());
    broker.fetch(input).map(|response| HttpFetchResponse {
        status: response.status,
        media_type: response.media_type,
        bytes: response.body.len(),
    })
}

#[derive(Clone)]
pub(super) struct GrantGate(Arc<AtomicU64>);

#[derive(Clone, Copy)]
struct GrantPermit(NonZeroU64);

impl GrantGate {
    pub(super) fn active() -> Self {
        Self(Arc::new(AtomicU64::new(1)))
    }

    fn issue(&self) -> Result<GrantPermit> {
        let revision = self.0.load(Ordering::Acquire);
        NonZeroU64::new(revision)
            .map(GrantPermit)
            .context("HTTP grant is revoked")
    }

    fn ensure_active(&self, permit: GrantPermit) -> Result<()> {
        ensure!(
            self.0.load(Ordering::Acquire) == permit.0.get(),
            "HTTP grant was revoked"
        );
        Ok(())
    }

    pub(super) fn revoke(&self) {
        self.0.store(0, Ordering::Release);
    }
}

#[derive(Default)]
struct HttpBudget {
    requests: usize,
    response_bytes: usize,
}

impl HttpBudget {
    fn begin_request(&mut self, policy: &HttpPolicy) -> Result<()> {
        self.requests = self
            .requests
            .checked_add(1)
            .context("HTTP request count overflow")?;
        ensure!(
            self.requests <= policy.maximum_requests(),
            "HTTP request quota exceeded"
        );
        Ok(())
    }

    fn record_bytes(&mut self, bytes: usize, policy: &HttpPolicy) -> Result<()> {
        self.response_bytes = self
            .response_bytes
            .checked_add(bytes)
            .context("HTTP response byte count overflow")?;
        ensure!(
            self.response_bytes <= policy.maximum_total_bytes(),
            "HTTP total byte quota exceeded"
        );
        Ok(())
    }
}

pub(super) struct HttpBroker<'a, R, A> {
    policy: &'a HttpPolicy,
    resolver: R,
    adapter: A,
    gate: GrantGate,
    budget: HttpBudget,
}

impl<'a, R: Resolver, A: Adapter> HttpBroker<'a, R, A> {
    pub(super) fn new(policy: &'a HttpPolicy, resolver: R, adapter: A, gate: GrantGate) -> Self {
        Self {
            policy,
            resolver,
            adapter,
            gate,
            budget: HttpBudget::default(),
        }
    }

    pub(super) fn fetch(&mut self, input: &str) -> Result<BrokerResponse> {
        let permit = self.gate.issue()?;
        let mut url = Url::parse(input).context("parse brokered HTTP URL")?;
        for redirects in 0..=self.policy.maximum_redirects() {
            self.gate.ensure_active(permit)?;
            let request = approve(&mut url, self.policy, &mut self.resolver)?;
            self.budget.begin_request(self.policy)?;
            let response = self.adapter.execute(request, self.policy)?;
            ensure!(
                response.header_bytes <= self.policy.maximum_response_header_bytes(),
                "HTTP response headers exceed configured limit"
            );
            if is_redirect(response.status) {
                ensure!(
                    redirects < self.policy.maximum_redirects(),
                    "HTTP redirect limit exceeded"
                );
                let location = response.location.context("HTTP redirect has no Location")?;
                url = url
                    .join(&location)
                    .context("resolve HTTP redirect target")?;
                continue;
            }
            ensure!(
                (200..300).contains(&response.status),
                "HTTP status is not successful"
            );
            let media_type = validate_media_type(response.media_type, self.policy)?;
            let body = read_body(
                response.body,
                response.content_length,
                &self.gate,
                permit,
                &mut self.budget,
                self.policy,
            )?;
            return Ok(BrokerResponse {
                status: response.status,
                media_type,
                body,
                redirects,
            });
        }
        Err(anyhow::anyhow!("HTTP redirect state exhausted"))
    }
}

fn approve<R: Resolver>(
    url: &mut Url,
    policy: &HttpPolicy,
    resolver: &mut R,
) -> Result<ApprovedRequest> {
    ensure!(url.scheme() == "https", "brokered HTTP requires HTTPS");
    ensure!(
        url.username().is_empty() && url.password().is_none(),
        "brokered HTTP rejects URL credentials"
    );
    ensure!(
        url.port_or_known_default() == Some(443),
        "brokered HTTP requires port 443"
    );
    url.set_fragment(None);
    let host = url
        .host_str()
        .context("brokered HTTP URL has no DNS host")?
        .to_ascii_lowercase();
    ensure!(
        policy
            .allowed_hosts()
            .iter()
            .any(|allowed| allowed == &host),
        "brokered HTTP host is outside the exact allowlist"
    );
    let mut addresses = resolver.resolve(&host, 443)?;
    addresses.sort_unstable();
    addresses.dedup();
    ensure!(
        !addresses.is_empty(),
        "brokered HTTP DNS returned no addresses"
    );
    ensure!(
        addresses.iter().all(|address| address.port() == 443),
        "brokered HTTP DNS returned an address with the wrong port"
    );
    let addresses_are_global = addresses.iter().try_fold(true, |accepted, address| {
        is_global(address.ip()).map(|is_global| accepted && is_global)
    })?;
    ensure!(
        addresses_are_global,
        "brokered HTTP DNS returned a non-global address"
    );
    Ok(ApprovedRequest {
        url: url.clone(),
        host,
        addresses: addresses.into_boxed_slice(),
    })
}

static PUBLIC_INTERNET_ACL: OnceLock<std::result::Result<HttpAcl, String>> = OnceLock::new();

fn is_global(address: IpAddr) -> Result<bool> {
    let acl = PUBLIC_INTERNET_ACL.get_or_init(|| {
        HttpAclBuilder::new()
            .ip_acl_default(true)
            .try_build()
            .map_err(|error| error.to_string())
    });
    match acl {
        Ok(acl) => Ok(acl.is_ip_allowed(&address).is_allowed()),
        Err(error) => Err(anyhow::anyhow!("construct public-IP classifier: {error}")),
    }
}

fn is_redirect(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

fn validate_media_type(value: Option<String>, policy: &HttpPolicy) -> Result<String> {
    let value = value.context("HTTP response has no Content-Type")?;
    let observed: Mime = value.parse().context("parse HTTP response Content-Type")?;
    let allowed = policy.allowed_media_types().iter().any(|allowed| {
        allowed.parse::<Mime>().is_ok_and(|candidate| {
            candidate.type_() == observed.type_() && candidate.subtype() == observed.subtype()
        })
    });
    ensure!(
        allowed,
        "HTTP response Content-Type is outside the allowlist"
    );
    Ok(observed.essence_str().to_owned())
}

fn read_body(
    mut reader: Box<dyn Read + Send>,
    content_length: Option<u64>,
    gate: &GrantGate,
    permit: GrantPermit,
    budget: &mut HttpBudget,
    policy: &HttpPolicy,
) -> Result<Vec<u8>> {
    if let Some(length) = content_length {
        ensure!(
            length <= u64::try_from(policy.maximum_response_bytes())?,
            "HTTP Content-Length exceeds response limit"
        );
    }
    let mut body = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        gate.ensure_active(permit)?;
        let read = reader.read(&mut chunk).context("read brokered HTTP body")?;
        if read == 0 {
            break;
        }
        gate.ensure_active(permit)?;
        let next = body
            .len()
            .checked_add(read)
            .context("HTTP body length overflow")?;
        ensure!(
            next <= policy.maximum_response_bytes(),
            "HTTP response body exceeds limit"
        );
        budget.record_bytes(read, policy)?;
        body.extend_from_slice(&chunk[..read]);
    }
    Ok(body)
}
