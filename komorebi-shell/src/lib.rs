//! Renderer-neutral shell domain primitives.

mod action_binding;
mod projection;
mod session;

pub use action_binding::ActionBinding;
pub use action_binding::ActionBindingError;
pub use action_binding::ActionInput;
pub use action_binding::ActionInputScalar;
pub use action_binding::BoundAction;
pub use projection::built_in_layout;
pub use session::ActionDispatchError;
pub use session::ActionDispatcher;
pub use session::ActionInvocationError;
pub use session::CommandSessionError;
pub use session::InvocationTicket;
pub use session::SessionLifetime;
pub use session::ShellSession;
pub use session::ShellSessionShutdownError;
pub use session::ShellSessionStartError;
