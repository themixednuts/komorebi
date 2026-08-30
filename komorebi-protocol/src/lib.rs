#![warn(clippy::all, clippy::pedantic)]

mod bootstrap;
mod error;
mod frame;
mod identity;
mod version;

pub use bootstrap::BootstrapCodec;
pub use bootstrap::BootstrapCodecError;
pub use bootstrap::FeatureId;
pub use bootstrap::HELLO_FRAME_KIND;
pub use bootstrap::Hello;
pub use bootstrap::RoleHint;
pub use error::FrameError;
pub use frame::Frame;
pub use frame::FrameHeader;
pub use frame::HEADER_BYTES;
pub use frame::MAX_FRAME_PAYLOAD_BYTES;
pub use identity::DirectionSequence;
pub use identity::FrameKind;
pub use identity::ProtocolPreface;
pub use identity::StreamId;
pub use version::CatalogSchemaVersion;
pub use version::ProtocolVersion;
pub use version::VersionRange;
pub use version::VersionRanges;
pub use version::VersionSetError;
