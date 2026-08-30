#![cfg(windows)]
#![warn(clippy::all, clippy::pedantic)]

mod endpoint;
mod error;
mod mailbox;
mod peer;
mod pipe;
mod security;
mod token;

pub use endpoint::CommandPipeEndpoint;
pub use endpoint::WindowsSessionId;
pub use error::TransportError;
pub use mailbox::LaneBuildError;
pub use mailbox::LaneMessage;
pub use mailbox::LanePublishError;
pub use mailbox::LanePublishFailure;
pub use mailbox::LanePublisher;
pub use mailbox::LaneReceiver;
pub use mailbox::SessionMailbox;
pub use mailbox::SessionMailboxPublishers;
pub use mailbox::SessionMailboxReceivers;
pub use mailbox::bounded_lane;
pub use mailbox::session_mailbox;
pub use peer::PeerIdentity;
pub use pipe::AuthenticatedPipe;
pub use pipe::CommandPipeListener;
pub use pipe::ProtocolConnection;
pub use token::LogonSid;
