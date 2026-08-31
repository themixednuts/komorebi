//! Renderer-neutral shell domain primitives.

mod action_binding;
mod command_palette;
mod palette_activation;
mod palette_controller;
mod projection;
mod session;
mod shortcut_guide;
mod web_activation;
mod web_search;
#[cfg(windows)]
mod windows_web;

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
pub use palette_activation::PaletteCompletion;
pub use palette_activation::PaletteCompletionDisposition;
pub use palette_activation::PaletteEffect;
pub use palette_activation::PaletteFailure;
pub use palette_activation::PaletteInvocation;
pub use palette_activation::PaletteSubmission;
pub use palette_activation::PaletteWebInvocation;
pub use palette_activation::PendingPaletteInvocation;
pub use palette_controller::PaletteAttemptId;
pub use palette_controller::PaletteContent;
pub use palette_controller::PaletteController;
pub use palette_controller::PaletteStatus;
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
pub use web_activation::WebActivationClient;
pub use web_activation::WebActivationCompletionError;
pub use web_activation::WebActivationQueueCapacity;
pub use web_activation::WebActivationService;
pub use web_activation::WebActivationShutdownError;
pub use web_activation::WebActivationSubmitError;
pub use web_activation::WebActivationTicket;
pub use web_activation::WebLaunchDisposition;
pub use web_activation::WebLaunchFailure;
pub use web_activation::WebSearchBroker;
pub use web_activation::WebUriLauncher;
pub use web_search::WebSearchEndpoint;
pub use web_search::WebSearchEndpointError;
pub use web_search::WebSearchTarget;
#[cfg(windows)]
pub use windows_web::WebUriSupport;
#[cfg(windows)]
pub use windows_web::WindowsWebLaunchError;
#[cfg(windows)]
pub use windows_web::WindowsWebLauncher;
