use serde::Serialize;
use windows::Devices::Radios::{Radio, RadioAccessStatus};
use windows::Devices::WiFi::WiFiAdapter;
use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager;
use windows::Networking::Connectivity::NetworkInformation;

#[derive(Debug, Serialize)]
pub struct WinRtProbe {
    pub radios: RouteObservation<Vec<RadioObservation>>,
    pub wifi_control_access: RouteObservation<i32>,
    pub media_sessions: RouteObservation<Vec<MediaSessionObservation>>,
    pub network: RouteObservation<NetworkObservation>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum RouteObservation<T> {
    Available { value: T },
    Denied { status: i32 },
    Unavailable { hresult: i32, message: String },
}

#[derive(Debug, Serialize)]
pub struct RadioObservation {
    pub name: String,
    pub kind: i32,
    pub state: i32,
}

#[derive(Debug, Serialize)]
pub struct MediaSessionObservation {
    pub source_app_user_model_id: String,
    pub playback_status: i32,
}

#[derive(Debug, Serialize)]
pub struct NetworkObservation {
    pub profile_name: String,
    pub connectivity_level: i32,
}

pub fn observe() -> WinRtProbe {
    WinRtProbe {
        radios: observe_radios(),
        wifi_control_access: observe_wifi_access(),
        media_sessions: observe_media_sessions(),
        network: observe_network(),
    }
}

fn observe_radios() -> RouteObservation<Vec<RadioObservation>> {
    capture((|| {
        let access = Radio::RequestAccessAsync()?.join()?;
        if access != RadioAccessStatus::Allowed {
            return Ok(RouteObservation::Denied { status: access.0 });
        }
        let radios = Radio::GetRadiosAsync()?.join()?;
        let mut observations = Vec::with_capacity(radios.Size()? as usize);
        for index in 0..radios.Size()? {
            let radio = radios.GetAt(index)?;
            observations.push(RadioObservation {
                name: radio.Name()?.to_string(),
                kind: radio.Kind()?.0,
                state: radio.State()?.0,
            });
        }
        Ok(RouteObservation::Available {
            value: observations,
        })
    })())
}

fn observe_wifi_access() -> RouteObservation<i32> {
    capture((|| {
        let status = WiFiAdapter::RequestAccessAsync()?.join()?;
        Ok(RouteObservation::Available { value: status.0 })
    })())
}

fn observe_media_sessions() -> RouteObservation<Vec<MediaSessionObservation>> {
    capture((|| {
        let manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()?.join()?;
        let sessions = manager.GetSessions()?;
        let mut observations = Vec::with_capacity(sessions.Size()? as usize);
        for index in 0..sessions.Size()? {
            let session = sessions.GetAt(index)?;
            observations.push(MediaSessionObservation {
                source_app_user_model_id: session.SourceAppUserModelId()?.to_string(),
                playback_status: session.GetPlaybackInfo()?.PlaybackStatus()?.0,
            });
        }
        Ok(RouteObservation::Available {
            value: observations,
        })
    })())
}

fn observe_network() -> RouteObservation<NetworkObservation> {
    capture((|| {
        let profile = NetworkInformation::GetInternetConnectionProfile()?;
        Ok(RouteObservation::Available {
            value: NetworkObservation {
                profile_name: profile.ProfileName()?.to_string(),
                connectivity_level: profile.GetNetworkConnectivityLevel()?.0,
            },
        })
    })())
}

fn capture<T>(result: windows::core::Result<RouteObservation<T>>) -> RouteObservation<T> {
    match result {
        Ok(observation) => observation,
        Err(error) => RouteObservation::Unavailable {
            hresult: error.code().0,
            message: error.message(),
        },
    }
}
