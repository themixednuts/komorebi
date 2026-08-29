use std::fmt;

use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ActionId(&'static str);

impl ActionId {
    pub const FOCUS_WINDOW: Self = Self("focus-window");
    pub const MOVE_WINDOW: Self = Self("move-window");
    pub const RESIZE_WINDOW: Self = Self("resize-window");
    pub const SET_WORKSPACE_LAYOUT: Self = Self("set-workspace-layout");
    pub const TOGGLE_WINDOW_FLOAT: Self = Self("toggle-window-float");

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

impl fmt::Display for ActionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ParameterId(&'static str);

impl ParameterId {
    pub const DIRECTION: Self = Self("direction");
    pub const AXIS: Self = Self("axis");
    pub const DELTA: Self = Self("delta");
    pub const WORKSPACE: Self = Self("workspace");
    pub const LAYOUT: Self = Self("layout");
    pub const WINDOW: Self = Self("window");

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ActionSchemaVersion(u16);

impl ActionSchemaVersion {
    pub const V1: Self = Self(1);

    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InvocationId(Uuid);

impl InvocationId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    #[must_use]
    pub const fn from_uuid(id: Uuid) -> Self {
        Self(id)
    }
}

impl Default for InvocationId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Revision(u64);

impl Revision {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    #[must_use]
    pub const fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PrincipalId(u64);

impl PrincipalId {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WindowId(u64);

impl WindowId {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
}

#[derive(Clone, Copy, Eq, PartialEq, Hash)]
pub struct ConfirmationToken([u8; 16]);

impl ConfirmationToken {
    #[must_use]
    pub fn issue() -> Self {
        Self(*Uuid::new_v4().as_bytes())
    }

    #[must_use]
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

impl fmt::Debug for ConfirmationToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ConfirmationToken([redacted])")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UndoToken(Uuid);

impl UndoToken {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for UndoToken {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmation_token_does_not_print_its_secret() {
        let token = ConfirmationToken::from_bytes([7; 16]);
        assert_eq!(format!("{token:?}"), "ConfirmationToken([redacted])");
    }

    #[test]
    fn action_ids_are_stable_leaves() {
        assert_eq!(ActionId::FOCUS_WINDOW.as_str(), "focus-window");
        assert_eq!(ActionId::MOVE_WINDOW.as_str(), "move-window");
        assert_eq!(ActionId::RESIZE_WINDOW.as_str(), "resize-window");
        assert_eq!(
            ActionId::SET_WORKSPACE_LAYOUT.as_str(),
            "set-workspace-layout"
        );
        assert_eq!(
            ActionId::TOGGLE_WINDOW_FLOAT.as_str(),
            "toggle-window-float"
        );
    }
}
