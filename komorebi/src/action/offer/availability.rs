use crate::action::BuiltinActionKind;

use super::ActionAuthority;
use super::ActionAvailability;
use super::ActionSnapshot;
use super::Unavailability;
use super::policy::DynamicChoiceSource;
use super::policy::FocusRequirement;
use super::policy::OfferPolicy;

pub(super) fn availability(
    snapshot: &ActionSnapshot,
    authority: &ActionAuthority,
    kind: BuiltinActionKind,
    policy: OfferPolicy,
) -> ActionAvailability {
    if !authority.grants.contains(kind) {
        return ActionAvailability::Unavailable(Unavailability::Unauthorized);
    }
    if kind == BuiltinActionKind::TogglePause {
        return ActionAvailability::Available;
    }
    if snapshot.paused {
        return ActionAvailability::Unavailable(Unavailability::ManagerPaused);
    }
    let focus = match policy.focus {
        FocusRequirement::None => ActionAvailability::Available,
        FocusRequirement::FocusedWindow => focused_window(snapshot),
        FocusRequirement::DirectionalTarget => directional_target(snapshot),
    };
    if focus != ActionAvailability::Available {
        return focus;
    }
    if policy.choices == DynamicChoiceSource::ExistingWorkspaceName
        && snapshot.named_workspaces.is_empty()
    {
        return ActionAvailability::Unavailable(Unavailability::UnknownWorkspace);
    }
    ActionAvailability::Available
}

fn focused_window(snapshot: &ActionSnapshot) -> ActionAvailability {
    if snapshot.focused_window.is_some() {
        ActionAvailability::Available
    } else {
        ActionAvailability::Unavailable(Unavailability::NoFocusedWindow)
    }
}

fn directional_target(snapshot: &ActionSnapshot) -> ActionAvailability {
    if snapshot.focused_window.is_none() {
        ActionAvailability::Unavailable(Unavailability::NoFocusedWindow)
    } else if !snapshot.directional_targets.is_empty() {
        ActionAvailability::Available
    } else {
        ActionAvailability::Unavailable(Unavailability::NoWindowInDirection)
    }
}
