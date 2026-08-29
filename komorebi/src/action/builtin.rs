use serde::Deserialize;
use serde::Serialize;
use thiserror::Error;

use crate::core::Axis;
use crate::core::DefaultLayout;
use crate::core::OperationDirection;

use super::id::ActionId;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum BuiltinActionKind {
    FocusWindow,
    MoveWindow,
    ResizeWindow,
    SetWorkspaceLayout,
    ToggleWindowFloat,
}

impl BuiltinActionKind {
    pub const ALL: [Self; 5] = [
        Self::FocusWindow,
        Self::MoveWindow,
        Self::ResizeWindow,
        Self::SetWorkspaceLayout,
        Self::ToggleWindowFloat,
    ];

    #[must_use]
    pub const fn id(self) -> ActionId {
        match self {
            Self::FocusWindow => ActionId::FOCUS_WINDOW,
            Self::MoveWindow => ActionId::MOVE_WINDOW,
            Self::ResizeWindow => ActionId::RESIZE_WINDOW,
            Self::SetWorkspaceLayout => ActionId::SET_WORKSPACE_LAYOUT,
            Self::ToggleWindowFloat => ActionId::TOGGLE_WINDOW_FLOAT,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum WorkspaceSelector {
    FocusedAtExecution,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum WindowSelector {
    FocusedAtExecution,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Pixels(i32);

impl Pixels {
    pub fn new(value: i32) -> Result<Self, PixelsError> {
        if value == 0 {
            return Err(PixelsError::Zero);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub const fn get(self) -> i32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Error)]
pub enum PixelsError {
    #[error("pixel delta must be non-zero")]
    Zero,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum BuiltinAction {
    FocusWindow {
        direction: OperationDirection,
    },
    MoveWindow {
        direction: OperationDirection,
    },
    ResizeWindow {
        axis: Axis,
        delta: Pixels,
    },
    SetWorkspaceLayout {
        workspace: WorkspaceSelector,
        layout: DefaultLayout,
    },
    ToggleWindowFloat {
        window: WindowSelector,
    },
}

impl PartialEq for BuiltinAction {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::FocusWindow { direction: left }, Self::FocusWindow { direction: right })
            | (Self::MoveWindow { direction: left }, Self::MoveWindow { direction: right }) => {
                std::mem::discriminant(left) == std::mem::discriminant(right)
            }
            (
                Self::ResizeWindow {
                    axis: left_axis,
                    delta: left_delta,
                },
                Self::ResizeWindow {
                    axis: right_axis,
                    delta: right_delta,
                },
            ) => left_axis == right_axis && left_delta == right_delta,
            (
                Self::SetWorkspaceLayout {
                    workspace: left_workspace,
                    layout: left_layout,
                },
                Self::SetWorkspaceLayout {
                    workspace: right_workspace,
                    layout: right_layout,
                },
            ) => left_workspace == right_workspace && left_layout == right_layout,
            (
                Self::ToggleWindowFloat {
                    window: left_window,
                },
                Self::ToggleWindowFloat {
                    window: right_window,
                },
            ) => left_window == right_window,
            _ => false,
        }
    }
}

impl Eq for BuiltinAction {}

impl BuiltinAction {
    #[must_use]
    pub const fn kind(self) -> BuiltinActionKind {
        match self {
            Self::FocusWindow { .. } => BuiltinActionKind::FocusWindow,
            Self::MoveWindow { .. } => BuiltinActionKind::MoveWindow,
            Self::ResizeWindow { .. } => BuiltinActionKind::ResizeWindow,
            Self::SetWorkspaceLayout { .. } => BuiltinActionKind::SetWorkspaceLayout,
            Self::ToggleWindowFloat { .. } => BuiltinActionKind::ToggleWindowFloat,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_has_one_stable_id() {
        let mut ids: Vec<_> = BuiltinActionKind::ALL
            .iter()
            .map(|kind| kind.id().as_str())
            .collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), BuiltinActionKind::ALL.len());
    }

    #[test]
    fn pixel_delta_rejects_zero() {
        assert_eq!(Pixels::new(0), Err(PixelsError::Zero));
        assert_eq!(Pixels::new(24).unwrap().get(), 24);
        assert_eq!(Pixels::new(-8).unwrap().get(), -8);
    }

    #[test]
    fn bound_action_round_trips_through_json() {
        let action = BuiltinAction::ResizeWindow {
            axis: Axis::Horizontal,
            delta: Pixels::new(16).unwrap(),
        };
        let encoded = serde_json::to_string(&action).unwrap();
        let decoded: BuiltinAction = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, action);
        assert_eq!(decoded.kind(), BuiltinActionKind::ResizeWindow);
    }
}
