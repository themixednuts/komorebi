mod argument;
mod codec;
mod contract;
mod lease;
mod lease_codec;

#[cfg(test)]
mod tests;

use crate::FrameKind;

pub const INVOKE_ACTION_FRAME_KIND: FrameKind = FrameKind::new(16);
pub const LEASE_INVOCATION_IDS_FRAME_KIND: FrameKind = FrameKind::new(17);
pub const INVOCATION_LEASE_REPLY_FRAME_KIND: FrameKind = FrameKind::new(18);

pub use argument::ActionArgument;
pub use argument::ActionArguments;
pub use argument::ArgumentError;
pub use argument::ArgumentScalar;
pub use argument::ArgumentScalars;
pub use argument::BoundedText;
pub use argument::ChoiceId;
pub use argument::Color;
pub use argument::EntityId;
pub use argument::EntityKind;
pub use argument::EntityReference;
pub use argument::FixedDecimal;
pub use argument::ParameterId;
pub use argument::SelectorId;
pub use argument::StableIdError;
pub use argument::Unit;
pub use argument::UnitValue;
pub use argument::WindowsPathInput;
pub use codec::ActionInvocationCodec;
pub use codec::CanonicalActionInvocation;
pub use codec::CommandCodecError;
pub use contract::ActionContractError;
pub use contract::ActionContractFingerprint;
pub use contract::ActionId;
pub use contract::ActionInvocation;
pub use contract::ActionKey;
pub use contract::ActionSchemaVersion;
pub use contract::CatalogStamp;
pub use contract::ConfirmationChallengeId;
pub use contract::OfferRef;
pub use contract::Revision;
pub use contract::StateStamp;
pub use lease::InvocationLeaseRejection;
pub use lease::InvocationLeaseReply;
pub use lease::InvocationLeaseRequest;
pub use lease_codec::InvocationLeaseCodec;
