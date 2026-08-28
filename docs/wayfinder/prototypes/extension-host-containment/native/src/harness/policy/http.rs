use std::num::NonZeroUsize;
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use super::nonzero_duration;

#[derive(Debug, Clone)]
pub(in crate::harness) struct HttpPolicy {
    allowed_hosts: Box<[String]>,
    allowed_media_types: Box<[String]>,
    maximum_redirects: usize,
    maximum_response_bytes: NonZeroUsize,
    maximum_total_bytes: NonZeroUsize,
    maximum_requests: NonZeroUsize,
    maximum_response_header_bytes: NonZeroUsize,
    timeout: Duration,
}

#[derive(Debug, Deserialize)]
pub(super) struct RawHttpPolicy {
    allowed_hosts: Vec<String>,
    allowed_media_types: Vec<String>,
    maximum_redirects: usize,
    maximum_response_bytes: usize,
    maximum_total_bytes: usize,
    maximum_requests: usize,
    maximum_response_header_bytes: usize,
    timeout_ms: u32,
}

impl TryFrom<RawHttpPolicy> for HttpPolicy {
    type Error = anyhow::Error;

    fn try_from(raw: RawHttpPolicy) -> Result<Self> {
        ensure!(
            !raw.allowed_hosts.is_empty()
                && raw.allowed_hosts.iter().all(|host| {
                    !host.is_empty()
                        && host == &host.to_ascii_lowercase()
                        && host
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
                }),
            "HTTP allowed_hosts must be exact lowercase DNS names"
        );
        ensure!(
            !raw.allowed_media_types.is_empty()
                && raw
                    .allowed_media_types
                    .iter()
                    .all(|value| value.parse::<mime::Mime>().is_ok()),
            "HTTP allowed_media_types must contain valid media types"
        );
        ensure!(
            raw.maximum_response_bytes <= raw.maximum_total_bytes,
            "maximum_response_bytes cannot exceed maximum_total_bytes"
        );
        Ok(Self {
            allowed_hosts: raw.allowed_hosts.into_boxed_slice(),
            allowed_media_types: raw.allowed_media_types.into_boxed_slice(),
            maximum_redirects: raw.maximum_redirects,
            maximum_response_bytes: NonZeroUsize::new(raw.maximum_response_bytes)
                .context("maximum_response_bytes must be nonzero")?,
            maximum_total_bytes: NonZeroUsize::new(raw.maximum_total_bytes)
                .context("maximum_total_bytes must be nonzero")?,
            maximum_requests: NonZeroUsize::new(raw.maximum_requests)
                .context("maximum_requests must be nonzero")?,
            maximum_response_header_bytes: NonZeroUsize::new(raw.maximum_response_header_bytes)
                .context("maximum_response_header_bytes must be nonzero")?,
            timeout: nonzero_duration(raw.timeout_ms, "HTTP timeout_ms")?,
        })
    }
}

impl HttpPolicy {
    pub(in crate::harness) fn allowed_hosts(&self) -> &[String] {
        &self.allowed_hosts
    }

    pub(in crate::harness) fn allowed_media_types(&self) -> &[String] {
        &self.allowed_media_types
    }

    pub(in crate::harness) const fn maximum_redirects(&self) -> usize {
        self.maximum_redirects
    }

    pub(in crate::harness) const fn maximum_response_bytes(&self) -> usize {
        self.maximum_response_bytes.get()
    }

    pub(in crate::harness) const fn maximum_total_bytes(&self) -> usize {
        self.maximum_total_bytes.get()
    }

    pub(in crate::harness) const fn maximum_requests(&self) -> usize {
        self.maximum_requests.get()
    }

    pub(in crate::harness) const fn maximum_response_header_bytes(&self) -> usize {
        self.maximum_response_header_bytes.get()
    }

    pub(in crate::harness) const fn timeout(&self) -> Duration {
        self.timeout
    }
}
