use std::collections::HashMap;
use std::time::Instant;

use thiserror::Error;

use super::builtin::BuiltinAction;
use super::id::ActionId;
use super::id::ConfirmationToken;
use super::id::PrincipalId;
use komorebi_protocol::StateStamp;

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
    #[error("confirmation token was issued for a different manager state")]
    StateMismatch,
}

#[derive(Clone, Debug)]
struct IssuedConfirmation {
    principal: PrincipalId,
    action_id: ActionId,
    canonical: String,
    state: StateStamp,
    expires_at: Instant,
    consumed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ValidatedConfirmation {
    key: [u8; 16],
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
        state: StateStamp,
        expires_at: Instant,
    ) {
        self.issued.insert(
            token_bytes(&token),
            IssuedConfirmation {
                principal,
                action_id: action.kind().id(),
                canonical: canonical_args(action),
                state,
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
        state: StateStamp,
        now: Instant,
    ) -> Result<(), ConfirmationError> {
        let validated = self.validate(token, principal, action, state, now)?;
        self.consume_validated(validated)
    }

    pub fn validate(
        &self,
        token: ConfirmationToken,
        principal: PrincipalId,
        action: &BuiltinAction,
        state: StateStamp,
        now: Instant,
    ) -> Result<ValidatedConfirmation, ConfirmationError> {
        let key = token_bytes(&token);
        let Some(issued) = self.issued.get(&key) else {
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
        if issued.state != state {
            return Err(ConfirmationError::StateMismatch);
        }
        Ok(ValidatedConfirmation { key })
    }

    pub fn consume_validated(
        &mut self,
        validated: ValidatedConfirmation,
    ) -> Result<(), ConfirmationError> {
        let Some(issued) = self.issued.get_mut(&validated.key) else {
            return Err(ConfirmationError::Unknown);
        };
        if issued.consumed {
            return Err(ConfirmationError::Replay);
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

    fn test_principal(byte: u8) -> PrincipalId {
        PrincipalId::new([byte; 32]).expect("test principal is nonzero")
    }
    use crate::action::WindowsPath;
    use crate::core::OperationDirection;
    use std::time::Duration;

    fn stamp_in(epoch: u8, revision: u64) -> StateStamp {
        StateStamp::new(
            komorebi_protocol::ManagerEpoch::new([epoch; 16]).expect("test epoch is non-nil"),
            komorebi_protocol::Revision::try_from(revision).expect("test revision is nonzero"),
        )
    }

    fn stamp(revision: u64) -> StateStamp {
        stamp_in(1, revision)
    }

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
    fn confirmation_rejects_changed_args_principal_state_expiry_and_replay() {
        let mut ledger = ConfirmationLedger::new();
        let token = ConfirmationToken::from_bytes([9; 16]);
        let principal = test_principal(7);
        let state = stamp(4);
        let now = Instant::now();
        ledger.issue(
            token,
            principal,
            &focus_left(),
            state,
            now + Duration::from_secs(30),
        );

        assert_eq!(
            ledger.consume(token, test_principal(8), &focus_left(), state, now),
            Err(ConfirmationError::PrincipalMismatch)
        );
        assert_eq!(
            ledger.consume(token, principal, &focus_right(), state, now),
            Err(ConfirmationError::ArgumentMismatch)
        );
        assert_eq!(
            ledger.consume(token, principal, &focus_left(), stamp(5), now),
            Err(ConfirmationError::StateMismatch)
        );
        assert_eq!(
            ledger.consume(token, principal, &focus_left(), stamp_in(2, 4), now),
            Err(ConfirmationError::StateMismatch)
        );
        assert_eq!(
            ledger.consume(
                token,
                principal,
                &focus_left(),
                state,
                now + Duration::from_secs(31)
            ),
            Err(ConfirmationError::Expired)
        );
        ledger
            .consume(token, principal, &focus_left(), state, now)
            .unwrap();
        assert_eq!(
            ledger.consume(token, principal, &focus_left(), state, now),
            Err(ConfirmationError::Replay)
        );
    }

    #[test]
    fn validation_is_read_only_until_the_proof_is_consumed() {
        let now = Instant::now();
        let token = ConfirmationToken::issue();
        let action = focus_left();
        let principal = test_principal(3);
        let state = stamp(4);
        let mut ledger = ConfirmationLedger::new();
        ledger.issue(
            token,
            principal,
            &action,
            state,
            now + Duration::from_secs(1),
        );

        let first = ledger
            .validate(token, principal, &action, state, now)
            .expect("confirmation should validate");
        let second = ledger
            .validate(token, principal, &action, state, now)
            .expect("validation must not consume the challenge");
        assert_eq!(first, second);
        ledger
            .consume_validated(first)
            .expect("validated proof should consume once");
        assert_eq!(
            ledger.consume_validated(second),
            Err(ConfirmationError::Replay)
        );
    }

    #[cfg(windows)]
    #[test]
    fn confirmation_preserves_wtf16_path_arguments() {
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;
        use std::path::PathBuf;

        let units = [b'C' as u16, b':' as u16, b'\\' as u16, 0xD800, b'x' as u16];
        let action = BuiltinAction::SetCustomLayout {
            path: WindowsPath::new(PathBuf::from(OsString::from_wide(&units))).unwrap(),
        };
        let mut ledger = ConfirmationLedger::new();
        let token = ConfirmationToken::from_bytes([3; 16]);
        let principal = test_principal(11);
        let state = stamp(7);
        let now = Instant::now();
        ledger.issue(
            token,
            principal,
            &action,
            state,
            now + Duration::from_secs(30),
        );

        assert_eq!(
            ledger.consume(token, principal, &action, state, now),
            Ok(())
        );
    }
}
