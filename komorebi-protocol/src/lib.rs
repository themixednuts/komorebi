#![warn(clippy::all, clippy::pedantic)]

mod error;
mod frame;
mod identity;

pub use error::FrameError;
pub use frame::Frame;
pub use frame::FrameHeader;
pub use identity::DirectionSequence;
pub use identity::FrameKind;
pub use identity::ProtocolPreface;
pub use identity::StreamId;
