use std::collections::HashMap;
use std::time::Instant;

use thiserror::Error;

use super::builtin::BuiltinAction;
use super::id::ActionId;
use super::id::ConfirmationToken;
use super::id::PrincipalId;
use super::id::Revision;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Error)]
pub enum ConfirmationError {
    #[error("confirmation token is unknown")]
    Unknown,
    #[error("confirmation token was already used")]
    Replay,
    #[error("confirmation token has expired")]
    Expired,
    #[error("confirmation token belongs to a different principal")]
    PrincipalMismatch,
    #[error("confirmation token was issued for a different action")]
    ActionMismatch,
    #[error("confirmation token was issued for different arguments")]
    ArgumentMismatch,
    #[error("confirmation token was issued for a different revision")]
    RevisionMismatch,
}

#[derive(Clone, Debug)]
struct IssuedConfirmation {
    principal: PrincipalId,
    action_id: ActionId,
    canonical: String,
    revision: Revision,
    expires_at: Instant,
    consumed: bool,
}

#[derive(Clone, Debug, Default)]
pub struct ConfirmationLedger {
    issued: HashMap<[u8; 16], IssuedConfirmation>,
}

impl ConfirmationLedger {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn issue(
        &mut self,
        token: ConfirmationToken,
        principal: PrincipalId,
        action: &BuiltinAction,
        revision: Revision,
        expires_at: Instant,
    ) {
        self.issued.insert(
            token_bytes(&token),
            IssuedConfirmation {
                principal,
                action_id: action.kind().id(),
                canonical: canonical_args(action),
                revision,
                expires_at,
                consumed: false,
            },
        );
    }

    pub fn consume(
        &mut self,
        token: ConfirmationToken,
        principal: PrincipalId,
        action: &BuiltinAction,
        revision: Revision,
        now: Instant,
    ) -> Result<(), ConfirmationError> {
        let Some(issued) = self.issued.get_mut(&token_bytes(&token)) else {
            return Err(ConfirmationError::Unknown);
        };
        if issued.consumed {
            return Err(ConfirmationError::Replay);
        }
        if now >= issued.expires_at {
            return Err(ConfirmationError::Expired);
        }
        if issued.principal != principal {
            return Err(ConfirmationError::PrincipalMismatch);
        }
        if issued.action_id != action.kind().id() {
            return Err(ConfirmationError::ActionMismatch);
        }
        if issued.canonical != canonical_args(action) {
            return Err(ConfirmationError::ArgumentMismatch);
        }
        if issued.revision != revision {
            return Err(ConfirmationError::RevisionMismatch);
        }
        issued.consumed = true;
        Ok(())
    }
}

fn canonical_args(action: &BuiltinAction) -> String {
    serde_json::to_string(action).expect("builtin actions are json-serializable")
}

fn token_bytes(token: &ConfirmationToken) -> [u8; 16] {
    *token.as_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::OperationDirection;
    use std::time::Duration;

    fn focus_left() -> BuiltinAction {
        BuiltinAction::FocusWindow {
            direction: OperationDirection::Left,
        }
    }

    fn focus_right() -> BuiltinAction {
        BuiltinAction::FocusWindow {
            direction: OperationDirection::Right,
        }
    }

    #[test]
    fn confirmation_rejects_changed_args_principal_revision_expiry_and_replay() {
        let mut ledger = ConfirmationLedger::new();
        let token = ConfirmationToken::from_bytes([9; 16]);
        let principal = PrincipalId::new(7);
        let revision = Revision::new(4);
        let now = Instant::now();
        ledger.issue(
            token,
            principal,
            &focus_left(),
            revision,
            now + Duration::from_secs(30),
        );

        assert_eq!(
            ledger.consume(token, PrincipalId::new(8), &focus_left(), revision, now),
            Err(ConfirmationError::PrincipalMismatch)
        );
        assert_eq!(
            ledger.consume(token, principal, &focus_right(), revision, now),
            Err(ConfirmationError::ArgumentMismatch)
        );
        assert_eq!(
            ledger.consume(token, principal, &focus_left(), Revision::new(5), now),
            Err(ConfirmationError::RevisionMismatch)
        );
        assert_eq!(
            ledger.consume(
                token,
                principal,
                &focus_left(),
                revision,
                now + Duration::from_secs(31)
            ),
            Err(ConfirmationError::Expired)
        );
        ledger
            .consume(token, principal, &focus_left(), revision, now)
            .unwrap();
        assert_eq!(
            ledger.consume(token, principal, &focus_left(), revision, now),
            Err(ConfirmationError::Replay)
        );
    }
}
