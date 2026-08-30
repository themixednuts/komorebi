#![cfg(windows)]
#![warn(clippy::all, clippy::pedantic)]

mod endpoint;
mod error;
mod peer;
mod pipe;
mod security;
mod token;

pub use endpoint::CommandPipeEndpoint;
pub use endpoint::WindowsSessionId;
pub use error::TransportError;
pub use peer::PeerIdentity;
pub use pipe::AuthenticatedPipe;
pub use pipe::CommandPipeListener;
pub use pipe::ProtocolConnection;
pub use token::LogonSid;
