use std::time::Duration;

use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ListenerAccess {
    Unspecified,
    Allowed,
    Denied,
    Unknown(i32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Added,
    Removed,
    Unknown(i32),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Utf16Text {
    pub units: Vec<u16>,
    pub display: String,
}

impl Utf16Text {
    #[must_use]
    pub fn from_units(units: &[u16]) -> Self {
        Self {
            units: units.to_vec(),
            display: String::from_utf16_lossy(units),
        }
    }

    #[must_use]
    pub fn contains_ascii(&self, needle: &str) -> bool {
        let needle: Vec<u16> = needle.encode_utf16().collect();
        !needle.is_empty()
            && self
                .units
                .windows(needle.len())
                .any(|window| window == needle)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ObservedNotification {
    pub id: u32,
    pub application_id: Utf16Text,
    pub application_name: Utf16Text,
    pub text: Vec<Utf16Text>,
}

impl ObservedNotification {
    #[must_use]
    pub fn contains_marker(&self, marker: &str) -> bool {
        self.text.iter().any(|text| text.contains_ascii(marker))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProducerPresentation {
    WindowsPopup,
    ProducerSuppressedPopup,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ObservationMeasurement {
    pub presentation: ProducerPresentation,
    pub added_after_micros: u128,
    pub removed_after_micros: u128,
    pub notification: ObservedNotification,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExclusivePresentationGate {
    Pass,
    Fail,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FocusSessionState {
    Unsupported,
    Inactive,
    Active,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExclusivePresentationBlocker {
    NoPreDisplayVeto,
    NoListenerActionInvocation,
    FocusMutationIsLimitedAccess,
    NoPermissionRevocationEvent,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CapabilityReport {
    pub listener_access: ListenerAccess,
    pub focus_session: FocusSessionState,
    pub exclusive_presentation_blockers: Vec<ExclusivePresentationBlocker>,
}

impl CapabilityReport {
    #[must_use]
    pub const fn exclusive_gate(&self) -> ExclusivePresentationGate {
        if self.exclusive_presentation_blockers.is_empty() {
            ExclusivePresentationGate::Pass
        } else {
            ExclusivePresentationGate::Fail
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectedMode {
    WindowsPresenterWithConsentedPrivateHistory,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProbeReport {
    pub capability: CapabilityReport,
    pub normal: ObservationMeasurement,
    pub producer_suppressed: ObservationMeasurement,
    pub deadline_millis: u128,
    pub exclusive_presentation: ExclusivePresentationGate,
    pub selected_mode: SelectedMode,
}

impl ProbeReport {
    #[must_use]
    pub fn new(
        capability: CapabilityReport,
        normal: ObservationMeasurement,
        producer_suppressed: ObservationMeasurement,
        deadline: Duration,
    ) -> Self {
        let exclusive_presentation = capability.exclusive_gate();
        Self {
            capability,
            normal,
            producer_suppressed,
            deadline_millis: deadline.as_millis(),
            exclusive_presentation,
            selected_mode: SelectedMode::WindowsPresenterWithConsentedPrivateHistory,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_matching_does_not_round_trip_through_utf8() {
        let text = Utf16Text::from_units(&[
            u16::from(b'a'),
            0xd800,
            u16::from(b'm'),
            u16::from(b'a'),
            u16::from(b'r'),
            u16::from(b'k'),
        ]);
        assert!(text.contains_ascii("mark"));
        assert_eq!(text.units[1], 0xd800);
    }

    #[test]
    fn exclusivity_fails_when_any_required_contract_is_absent() {
        let capability = CapabilityReport {
            listener_access: ListenerAccess::Allowed,
            focus_session: FocusSessionState::Inactive,
            exclusive_presentation_blockers: vec![ExclusivePresentationBlocker::NoPreDisplayVeto],
        };
        assert_eq!(capability.exclusive_gate(), ExclusivePresentationGate::Fail);
    }
}
