use super::ActionUnavailability;
use super::CatalogStamp;
use super::InvocationStatus;
use super::StateStamp;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InvocationSubmissionReply {
    Accepted(InvocationStatus),
    Retained(InvocationStatus),
    Rejected(InvocationRejection),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InvocationRejection {
    Unauthorized,
    IdempotencyConflict,
    InvocationExpired,
    InvocationNotLeased,
    UnknownNamespace,
    CapacityFull,
    StaleEpoch,
    StaleState { current: StateStamp },
    StaleCatalog { current: CatalogStamp },
    StaleOffer,
    InvalidArguments,
    Unavailable(ActionUnavailability),
    ConfirmationRequired,
    ConfirmationUnavailable,
}
