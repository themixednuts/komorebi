use super::ActionArguments;
use super::ActionId;

/// An action and its typed arguments before catalog binding and authorization.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionIntent {
    action: ActionId,
    arguments: ActionArguments,
}

impl ActionIntent {
    #[must_use]
    pub const fn new(action: ActionId, arguments: ActionArguments) -> Self {
        Self { action, arguments }
    }

    #[must_use]
    pub const fn action(&self) -> &ActionId {
        &self.action
    }

    #[must_use]
    pub const fn arguments(&self) -> &ActionArguments {
        &self.arguments
    }

    #[must_use]
    pub fn into_parts(self) -> (ActionId, ActionArguments) {
        (self.action, self.arguments)
    }
}
