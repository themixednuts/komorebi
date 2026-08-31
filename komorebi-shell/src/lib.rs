//! Renderer-neutral shell domain primitives.

mod action_binding;
mod command_palette;
mod projection;
mod session;
mod shortcut_guide;

pub use action_binding::ActionBinding;
pub use action_binding::ActionBindingError;
pub use action_binding::ActionInput;
pub use action_binding::ActionInputScalar;
pub use action_binding::BoundAction;
pub use command_palette::CommandPalette;
pub use command_palette::PaletteAction;
pub use command_palette::PaletteActionState;
pub use command_palette::PaletteMatches;
pub use command_palette::PaletteQuery;
pub use command_palette::PaletteResults;
pub use command_palette::PaletteSearchTerms;
pub use command_palette::PaletteSelectionMove;
pub use command_palette::WebSearchRequest;
pub use command_palette::WebSearchTerms;
pub use projection::built_in_layout;
pub use session::ActionInvocationError;
pub use session::CatalogReadError;
pub use session::CatalogTicket;
pub use session::CommandSessionError;
pub use session::InvocationTicket;
pub use session::SessionLifetime;
pub use session::ShellHandle;
pub use session::ShellRequestError;
pub use session::ShellSession;
pub use session::ShellSessionShutdownError;
pub use session::ShellSessionStartError;
pub use shortcut_guide::ShortcutGuide;
pub use shortcut_guide::ShortcutGuideEntry;
