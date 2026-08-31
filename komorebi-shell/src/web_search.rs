use thiserror::Error;
use url::Url;

use crate::WebSearchRequest;

/// A configured HTTPS search endpoint with one unambiguous terms parameter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebSearchEndpoint {
    base: Url,
    query_parameter: Box<str>,
}

impl WebSearchEndpoint {
    /// Validates a configured search endpoint.
    ///
    /// # Errors
    ///
    /// Rejects malformed or non-HTTPS URLs, ambient credentials, fragments,
    /// invalid parameter names, and endpoints that already contain the terms
    /// parameter.
    pub fn new(base: &str, query_parameter: &str) -> Result<Self, WebSearchEndpointError> {
        let base = Url::parse(base)?;
        if base.scheme() != "https" {
            return Err(WebSearchEndpointError::HttpsRequired);
        }
        if base.host_str().is_none() {
            return Err(WebSearchEndpointError::HostRequired);
        }
        if !base.username().is_empty() || base.password().is_some() {
            return Err(WebSearchEndpointError::CredentialsForbidden);
        }
        if base.fragment().is_some() {
            return Err(WebSearchEndpointError::FragmentForbidden);
        }
        if !valid_query_parameter(query_parameter) {
            return Err(WebSearchEndpointError::InvalidQueryParameter);
        }
        if base.query_pairs().any(|(name, _)| name == query_parameter) {
            return Err(WebSearchEndpointError::DuplicateQueryParameter);
        }
        Ok(Self {
            base,
            query_parameter: query_parameter.into(),
        })
    }

    /// Builds the only URI authorized by this endpoint for the request.
    #[must_use]
    pub fn target(&self, request: &WebSearchRequest) -> WebSearchTarget {
        let mut target = self.base.clone();
        target
            .query_pairs_mut()
            .append_pair(&self.query_parameter, request.terms());
        WebSearchTarget(target)
    }

    /// Returns the canonical configured base URL for persistence.
    #[must_use]
    pub fn base_url(&self) -> &str {
        self.base.as_str()
    }

    /// Returns the validated query key for persistence.
    #[must_use]
    pub fn query_parameter(&self) -> &str {
        &self.query_parameter
    }
}

fn valid_query_parameter(parameter: &str) -> bool {
    !parameter.is_empty()
        && parameter
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~'))
}

/// An HTTPS URI constructed by a validated search endpoint.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebSearchTarget(Url);

impl WebSearchTarget {
    /// Returns the URI scheme, which is always `https` for constructed values.
    #[must_use]
    pub fn scheme(&self) -> &str {
        self.0.scheme()
    }

    /// Returns the validated endpoint host.
    #[must_use]
    pub fn host(&self) -> Option<&str> {
        self.0.host_str()
    }

    /// Returns the fully encoded launch URI.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// Invalid web-search endpoint configuration.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum WebSearchEndpointError {
    /// The configured value is not an absolute URL.
    #[error("invalid web-search URL: {0}")]
    InvalidUrl(#[from] url::ParseError),
    /// Search targets must use transport-secured HTTP.
    #[error("web-search endpoint must use HTTPS")]
    HttpsRequired,
    /// Search targets require a network host.
    #[error("web-search endpoint must contain a host")]
    HostRequired,
    /// User information must never be inherited by generated targets.
    #[error("web-search endpoint must not contain credentials")]
    CredentialsForbidden,
    /// Fragments are ambiguous once a query is appended.
    #[error("web-search endpoint must not contain a fragment")]
    FragmentForbidden,
    /// The query key must be a nonempty URI-unreserved ASCII name.
    #[error("web-search query parameter is invalid")]
    InvalidQueryParameter,
    /// A configured endpoint must leave its terms parameter unset.
    #[error("web-search endpoint already contains its query parameter")]
    DuplicateQueryParameter,
}
