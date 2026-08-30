#![cfg(windows)]
#![warn(clippy::all, clippy::pedantic)]

mod endpoint;
mod error;
mod mailbox;
mod peer;
mod pipe;
mod security;
mod server;
mod subscription;
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
pub use server::CommandProtocolServer;
pub use server::EstablishedSession;
pub use server::PendingProtocolSession;
pub use server::SessionAcceptance;
pub use subscription::EventDelivery;
pub use subscription::EventSubscriptions;
pub use subscription::ResumeDecision;
pub use subscription::ResumeStart;
pub use subscription::SubscriberClass;
pub use subscription::SubscriptionControl;
pub use subscription::SubscriptionError;
pub use subscription::SubscriptionStart;
pub use token::LogonSid;
