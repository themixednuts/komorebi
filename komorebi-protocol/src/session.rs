use thiserror::Error;

mod authority;
mod limits;
mod negotiation;

pub use authority::AuthorityCapabilityId;
pub use authority::AuthoritySummary;
pub use authority::AuthoritySummaryError;
pub use authority::CommandCapability;
pub use limits::AssemblyDeadlineMs;
pub use limits::ChunkPayloadLimit;
pub use limits::ControlPayloadLimit;
pub use limits::FramePayloadLimit;
pub use limits::NestingLimit;
pub use limits::ReassemblyLimit;
pub use limits::SessionLimitError;
pub use limits::SessionLimits;
pub use negotiation::NegotiatedProtocol;
pub use negotiation::NegotiationError;
pub use negotiation::ProtocolNegotiator;
pub use negotiation::ServerSupport;

macro_rules! opaque_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
        pub struct $name([u8; 16]);

        impl $name {
            /// Creates a non-nil opaque session identity.
            ///
            /// # Errors
            ///
            /// Returns [`IdentifierError::Nil`] for the all-zero identity.
            pub fn new(bytes: [u8; 16]) -> Result<Self, IdentifierError> {
                if bytes == [0; 16] {
                    Err(IdentifierError::Nil)
                } else {
                    Ok(Self(bytes))
                }
            }

            #[must_use]
            pub const fn into_bytes(self) -> [u8; 16] {
                self.0
            }
        }
    };
}

opaque_id!(ManagerEpoch);
opaque_id!(ConnectionId);
opaque_id!(TraceId);

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum IdentifierError {
    #[error("session identities must not be nil")]
    Nil,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Welcome {
    negotiated: NegotiatedProtocol,
    manager_epoch: ManagerEpoch,
    connection_id: ConnectionId,
    authority_summary: AuthoritySummary,
}

impl Welcome {
    #[must_use]
    pub fn new(
        negotiated: NegotiatedProtocol,
        manager_epoch: ManagerEpoch,
        connection_id: ConnectionId,
        authority_summary: AuthoritySummary,
    ) -> Self {
        Self {
            negotiated,
            manager_epoch,
            connection_id,
            authority_summary,
        }
    }

    #[must_use]
    pub const fn negotiated(&self) -> &NegotiatedProtocol {
        &self.negotiated
    }

    #[must_use]
    pub const fn manager_epoch(&self) -> ManagerEpoch {
        self.manager_epoch
    }

    #[must_use]
    pub const fn connection_id(&self) -> ConnectionId {
        self.connection_id
    }

    #[must_use]
    pub const fn authority_summary(&self) -> &AuthoritySummary {
        &self.authority_summary
    }
}

#[cfg(test)]
mod tests;
