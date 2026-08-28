use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use windows::ApplicationModel::AppInfo;
use windows::Data::Xml::Dom::XmlDocument;
use windows::Foundation::TypedEventHandler;
use windows::UI::Notifications::Management::UserNotificationListener;
use windows::UI::Notifications::{
    KnownNotificationBindings, ToastNotification, ToastNotificationManager, UserNotification,
    UserNotificationChangedEventArgs, UserNotificationChangedKind,
};
use windows::UI::Shell::FocusSessionManager;
use windows::Win32::System::WinRT::{RO_INIT_SINGLETHREADED, RoInitialize, RoUninitialize};
use windows::core::{HSTRING, Result as WindowsResult};

use crate::model::{
    CapabilityReport, ChangeKind, ExclusivePresentationBlocker, FocusSessionState, ListenerAccess,
    ObservationMeasurement, ObservedNotification, ProducerPresentation, Utf16Text,
};

#[derive(Debug, thiserror::Error)]
pub enum ProbeError {
    #[error("Windows API failed: {0}")]
    Windows(#[from] windows::core::Error),
    #[error("notification listener access is {0:?}")]
    ListenerUnavailable(ListenerAccess),
    #[error("no matching notification event arrived before the operation deadline")]
    Deadline,
    #[error("the notification event channel disconnected")]
    EventChannelDisconnected,
    #[error("notification listener event omitted its event arguments")]
    MissingEventArguments,
}

#[derive(Clone, Copy, Debug)]
struct ListenerEvent {
    notification_id: u32,
    kind: ChangeKind,
    observed_at: Instant,
}

pub struct NotificationProbe {
    listener: UserNotificationListener,
    events: Receiver<Result<ListenerEvent, ProbeError>>,
    event_token: i64,
    deadline: Duration,
    _apartment: StaApartment,
}

struct StaApartment;

impl StaApartment {
    fn initialize() -> WindowsResult<Self> {
        // SAFETY: Called once for this probe's primary thread before any WinRT object is created.
        unsafe { RoInitialize(RO_INIT_SINGLETHREADED)? };
        Ok(Self)
    }
}

impl Drop for StaApartment {
    fn drop(&mut self) {
        // SAFETY: Balances this thread's successful RoInitialize call after WinRT fields drop.
        unsafe { RoUninitialize() };
    }
}

impl NotificationProbe {
    /// Creates an event-driven probe on the calling STA thread.
    ///
    /// # Errors
    ///
    /// Returns a typed error if COM initialization or listener event registration fails.
    pub fn connect(deadline: Duration) -> Result<Self, ProbeError> {
        let apartment = StaApartment::initialize()?;
        let listener = UserNotificationListener::Current()?;
        let (sender, events) = mpsc::channel();
        let event_token = listener.NotificationChanged(&TypedEventHandler::new(
            move |_listener, arguments: windows::core::Ref<UserNotificationChangedEventArgs>| {
                let result = (|| {
                    let arguments = arguments
                        .as_ref()
                        .ok_or(ProbeError::MissingEventArguments)?;
                    let kind = map_change_kind(arguments.ChangeKind()?);
                    let notification_id = arguments.UserNotificationId()?;
                    Ok(ListenerEvent {
                        notification_id,
                        kind,
                        observed_at: Instant::now(),
                    })
                })();
                // The receiver owns experiment lifetime; a late callback after teardown is benign.
                let _send_result = sender.send(result);
                Ok(())
            },
        ))?;
        Ok(Self {
            listener,
            events,
            event_token,
            deadline,
            _apartment: apartment,
        })
    }

    /// Requests the privacy-sensitive notification-listener permission from the UI thread.
    ///
    /// # Errors
    ///
    /// Returns the Windows error if the consent operation cannot be started or completed.
    pub fn request_access(&self) -> Result<ListenerAccess, ProbeError> {
        Ok(map_access(self.listener.RequestAccessAsync()?.join()?))
    }

    /// Reports supported public contracts without treating absence as notification absence.
    ///
    /// # Errors
    ///
    /// Returns the Windows error if the listener or Focus state cannot be read.
    pub fn capability_report(&self) -> Result<CapabilityReport, ProbeError> {
        let focus_supported = FocusSessionManager::IsSupported()?;
        let focus_session = if focus_supported {
            let focus = FocusSessionManager::GetDefault()?;
            if focus.IsFocusActive()? {
                FocusSessionState::Active
            } else {
                FocusSessionState::Inactive
            }
        } else {
            FocusSessionState::Unsupported
        };
        Ok(CapabilityReport {
            listener_access: map_access(self.listener.GetAccessStatus()?),
            focus_session,
            exclusive_presentation_blockers: vec![
                // NotificationChanged observes Added/Removed after Windows accepts a notification.
                ExclusivePresentationBlocker::NoPreDisplayVeto,
                // UserNotification has content and dismissal but no original-action invocation.
                ExclusivePresentationBlocker::NoListenerActionInvocation,
                // Start/deactivate Focus require a Limited Access Feature token.
                ExclusivePresentationBlocker::FocusMutationIsLimitedAccess,
                // The listener has no access-status change event.
                ExclusivePresentationBlocker::NoPermissionRevocationEvent,
            ],
        })
    }

    /// Measures one producer-owned presentation policy and listener dismissal round trip.
    ///
    /// # Errors
    ///
    /// Returns a typed error when access is unavailable, Windows rejects an operation, or the
    /// bounded native-event wait expires.
    pub fn measure(
        &self,
        marker: &str,
        presentation: ProducerPresentation,
    ) -> Result<ObservationMeasurement, ProbeError> {
        let access = map_access(self.listener.GetAccessStatus()?);
        if access != ListenerAccess::Allowed {
            return Err(ProbeError::ListenerUnavailable(access));
        }

        let toast = build_toast(marker, presentation)?;
        let notifier = ToastNotificationManager::CreateToastNotifier()?;
        let shown_at = Instant::now();
        notifier.Show(&toast)?;

        let (added, notification) = self.wait_for_marker(marker, ChangeKind::Added, shown_at)?;
        let dismiss_started = Instant::now();
        self.listener.RemoveNotification(notification.id)?;
        let removed = self.wait_for_id(notification.id, ChangeKind::Removed, dismiss_started)?;
        Ok(ObservationMeasurement {
            presentation,
            added_after_micros: added.observed_at.duration_since(shown_at).as_micros(),
            removed_after_micros: removed
                .observed_at
                .duration_since(dismiss_started)
                .as_micros(),
            notification,
        })
    }

    fn wait_for_marker(
        &self,
        marker: &str,
        expected: ChangeKind,
        started: Instant,
    ) -> Result<(ListenerEvent, ObservedNotification), ProbeError> {
        loop {
            let event = self.receive_before(started)?;
            if event.kind != expected {
                continue;
            }
            let observed = observe_notification(&self.listener, event.notification_id)?;
            if observed.contains_marker(marker) {
                return Ok((event, observed));
            }
        }
    }

    fn wait_for_id(
        &self,
        id: u32,
        expected: ChangeKind,
        started: Instant,
    ) -> Result<ListenerEvent, ProbeError> {
        loop {
            let event = self.receive_before(started)?;
            if event.notification_id == id && event.kind == expected {
                return Ok(event);
            }
        }
    }

    fn receive_before(&self, started: Instant) -> Result<ListenerEvent, ProbeError> {
        let elapsed = started.elapsed();
        let remaining = self
            .deadline
            .checked_sub(elapsed)
            .ok_or(ProbeError::Deadline)?;
        match self.events.recv_timeout(remaining) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => Err(ProbeError::Deadline),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(ProbeError::EventChannelDisconnected),
        }
    }
}

impl Drop for NotificationProbe {
    fn drop(&mut self) {
        // Unregistration failure cannot be recovered during Drop; process teardown removes the
        // callback, and no manager-owned presentation authority was acquired.
        let _remove_result = self.listener.RemoveNotificationChanged(self.event_token);
    }
}

fn build_toast(
    marker: &str,
    presentation: ProducerPresentation,
) -> WindowsResult<ToastNotification> {
    let document = XmlDocument::new()?;
    let escaped = escape_xml(marker);
    let xml = format!(
        "<toast><visual><binding template=\"ToastGeneric\"><text>komorebi notification probe</text><text>{escaped}</text></binding></visual></toast>"
    );
    document.LoadXml(&HSTRING::from(xml))?;
    let toast = ToastNotification::CreateToastNotification(&document)?;
    toast.SetSuppressPopup(presentation == ProducerPresentation::ProducerSuppressedPopup)?;
    Ok(toast)
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn observe_notification(
    listener: &UserNotificationListener,
    id: u32,
) -> Result<ObservedNotification, ProbeError> {
    let notification = listener.GetNotification(id)?;
    let app = notification.AppInfo()?;
    let application_id = hstring_text(&app.AppUserModelId()?);
    let application_name = app_name(&app)?;
    let text = notification_text(&notification)?;
    Ok(ObservedNotification {
        id,
        application_id,
        application_name,
        text,
    })
}

fn app_name(app: &AppInfo) -> WindowsResult<Utf16Text> {
    Ok(hstring_text(&app.DisplayInfo()?.DisplayName()?))
}

fn notification_text(notification: &UserNotification) -> WindowsResult<Vec<Utf16Text>> {
    let binding = notification
        .Notification()?
        .Visual()?
        .GetBinding(&KnownNotificationBindings::ToastGeneric()?)?;
    let elements = binding.GetTextElements()?;
    let mut text = Vec::new();
    for element in elements {
        text.push(hstring_text(&element.Text()?));
    }
    Ok(text)
}

fn hstring_text(value: &HSTRING) -> Utf16Text {
    Utf16Text::from_units(value)
}

const fn map_access(
    status: windows::UI::Notifications::Management::UserNotificationListenerAccessStatus,
) -> ListenerAccess {
    match status.0 {
        0 => ListenerAccess::Unspecified,
        1 => ListenerAccess::Allowed,
        2 => ListenerAccess::Denied,
        value => ListenerAccess::Unknown(value),
    }
}

const fn map_change_kind(status: UserNotificationChangedKind) -> ChangeKind {
    match status.0 {
        0 => ChangeKind::Added,
        1 => ChangeKind::Removed,
        value => ChangeKind::Unknown(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_escaping_is_complete_for_text_nodes() {
        assert_eq!(escape_xml("<&>\"'"), "&lt;&amp;&gt;&quot;&apos;");
    }
}
