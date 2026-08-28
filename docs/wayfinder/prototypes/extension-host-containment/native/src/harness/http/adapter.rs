use std::io::Read;

use anyhow::{Context, Result};
use reqwest::blocking::Client;
use reqwest::header::{ACCEPT, CONTENT_TYPE, LOCATION};

use super::{Adapter, ApprovedRequest, RawResponse};
use crate::harness::policy::HttpPolicy;

pub(super) struct ReqwestAdapter;

impl Adapter for ReqwestAdapter {
    fn execute(&mut self, request: ApprovedRequest, policy: &HttpPolicy) -> Result<RawResponse> {
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .referer(false)
            .no_proxy()
            .https_only(true)
            .tls_version_min(reqwest::tls::Version::TLS_1_2)
            .http1_only()
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd()
            .timeout(policy.timeout())
            .connect_timeout(policy.timeout())
            .pool_max_idle_per_host(0)
            .resolve_to_addrs(&request.host, &request.addresses)
            .user_agent("komorebi-wayfinder/0")
            .build()
            .context("build pinned brokered HTTP client")?;
        let response = client
            .get(request.url)
            .header(ACCEPT, policy.allowed_media_types().join(", "))
            .send()
            .context("execute pinned brokered HTTP request")?;
        let status = response.status().as_u16();
        let location = header_text(response.headers().get(LOCATION), "Location")?;
        let media_type = header_text(response.headers().get(CONTENT_TYPE), "Content-Type")?;
        let content_length = response.content_length();
        let header_bytes =
            response
                .headers()
                .iter()
                .try_fold(0_usize, |total, (name, value)| {
                    total
                        .checked_add(name.as_str().len())
                        .and_then(|bytes| bytes.checked_add(value.as_bytes().len()))
                        .and_then(|bytes| bytes.checked_add(4))
                        .context("HTTP response header size overflow")
                })?;
        Ok(RawResponse {
            status,
            location,
            media_type,
            content_length,
            header_bytes,
            body: Box::new(response) as Box<dyn Read + Send>,
        })
    }
}

fn header_text(value: Option<&reqwest::header::HeaderValue>, name: &str) -> Result<Option<String>> {
    value
        .map(|value| {
            value
                .to_str()
                .with_context(|| format!("HTTP {name} is not valid visible text"))
                .map(str::to_owned)
        })
        .transpose()
}
