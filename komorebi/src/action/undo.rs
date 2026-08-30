use std::collections::HashSet;

use thiserror::Error;

use super::definition::UndoPolicy;
use super::id::UndoToken;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Error)]
pub enum UndoError {
    #[error("action does not support undo")]
    Unsupported,
    #[error("undo token was already consumed")]
    Consumed,
    #[error("undo token is unknown")]
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UndoRecord {
    pub token: UndoToken,
    pub policy: UndoPolicy,
}

#[derive(Clone, Debug, Default)]
pub struct UndoLedger {
    issued: HashSet<UndoToken>,
    consumed: HashSet<UndoToken>,
}

impl UndoLedger {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn issue(&mut self, policy: UndoPolicy) -> Result<UndoRecord, UndoError> {
        let record = Self::prepare(policy)?;
        self.commit(record);
        Ok(record)
    }

    pub fn prepare(policy: UndoPolicy) -> Result<UndoRecord, UndoError> {
        match policy {
            UndoPolicy::None => Err(UndoError::Unsupported),
            UndoPolicy::PriorManagerIntent | UndoPolicy::ExactCapturedState => Ok(UndoRecord {
                token: UndoToken::new(),
                policy,
            }),
        }
    }

    pub fn commit(&mut self, record: UndoRecord) {
        self.issued.insert(record.token);
    }

    pub fn consume(&mut self, token: UndoToken) -> Result<(), UndoError> {
        if self.consumed.contains(&token) {
            return Err(UndoError::Consumed);
        }
        if !self.issued.contains(&token) {
            return Err(UndoError::Unknown);
        }
        self.consumed.insert(token);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_records_are_issued_only_for_declared_policies() {
        let mut ledger = UndoLedger::new();
        assert_eq!(ledger.issue(UndoPolicy::None), Err(UndoError::Unsupported));
        let record = ledger.issue(UndoPolicy::PriorManagerIntent).unwrap();
        ledger.consume(record.token).unwrap();
        assert_eq!(ledger.consume(record.token), Err(UndoError::Consumed));
        assert_eq!(ledger.consume(UndoToken::new()), Err(UndoError::Unknown));
    }

    #[test]
    fn prepared_undo_is_unknown_until_committed() {
        let mut ledger = UndoLedger::new();
        let record = UndoLedger::prepare(UndoPolicy::PriorManagerIntent).unwrap();
        assert_eq!(ledger.consume(record.token), Err(UndoError::Unknown));
        ledger.commit(record);
        assert_eq!(ledger.consume(record.token), Ok(()));
    }
}
