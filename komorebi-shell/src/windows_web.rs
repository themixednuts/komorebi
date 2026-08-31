use thiserror::Error;
use windows::Foundation::Uri;
use windows::System::LaunchQuerySupportStatus;
use windows::System::LaunchQuerySupportType;
use windows::System::Launcher;
use windows::core::HSTRING;

use crate::WebLaunchDisposition;
use crate::WebLaunchFailure;
use crate::WebSearchTarget;
use crate::WebUriLauncher;

/// Windows' current support state for an HTTPS URI association.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebUriSupport {
    Available,
    AppNotInstalled,
    AppUnavailable,
    NotSupported,
    Unknown,
}

/// Native Windows URI activation through the user's default application.
#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsWebLauncher;

impl WindowsWebLauncher {
    /// Queries the registered HTTPS handler without launching it.
    ///
    /// # Errors
    ///
    /// Returns a projected Windows Runtime failure if URI construction or the
    /// native asynchronous query fails.
    pub async fn query_support(
        target: &WebSearchTarget,
    ) -> Result<WebUriSupport, WindowsWebLaunchError> {
        let uri = native_uri(target)?;
        let status = Launcher::QueryUriSupportAsync(&uri, LaunchQuerySupportType::Uri)?.await?;
        Ok(match status {
            LaunchQuerySupportStatus::Available => WebUriSupport::Available,
            LaunchQuerySupportStatus::AppNotInstalled => WebUriSupport::AppNotInstalled,
            LaunchQuerySupportStatus::AppUnavailable => WebUriSupport::AppUnavailable,
            LaunchQuerySupportStatus::NotSupported => WebUriSupport::NotSupported,
            _ => WebUriSupport::Unknown,
        })
    }

    /// Launches the registered default application for a user-initiated URI.
    ///
    /// The caller must invoke this only as a direct consequence of foreground
    /// user input, as required by `Windows.System.Launcher`.
    ///
    /// # Errors
    ///
    /// Returns a projected Windows Runtime failure if URI construction or the
    /// native asynchronous launch fails.
    pub async fn launch(
        target: &WebSearchTarget,
    ) -> Result<WebLaunchDisposition, WindowsWebLaunchError> {
        let uri = native_uri(target)?;
        Ok(if Launcher::LaunchUriAsync(&uri)?.await? {
            WebLaunchDisposition::Launched
        } else {
            WebLaunchDisposition::Rejected
        })
    }
}

impl WebUriLauncher for WindowsWebLauncher {
    async fn launch(
        &self,
        target: WebSearchTarget,
    ) -> Result<WebLaunchDisposition, WebLaunchFailure> {
        Self::launch(&target).await.map_err(WebLaunchFailure::from)
    }
}

fn native_uri(target: &WebSearchTarget) -> Result<Uri, windows::core::Error> {
    Uri::CreateUri(&HSTRING::from(target.as_str()))
}

/// Failure returned by Windows Runtime URI activation.
#[derive(Clone, Debug, Error)]
#[error("Windows URI activation failed: {0}")]
pub struct WindowsWebLaunchError(#[from] windows::core::Error);

impl From<WindowsWebLaunchError> for WebLaunchFailure {
    fn from(error: WindowsWebLaunchError) -> Self {
        Self::native(error.0.code().0, error.to_string())
    }
}
