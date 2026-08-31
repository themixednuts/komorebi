//! Renderer-neutral shell domain primitives.

mod action_binding;
mod action_input;
mod application_activation;
mod application_catalog;
mod bound_action;
mod command_palette;
mod file_activation;
mod palette_activation;
mod palette_controller;
mod palette_search;
mod projection;
mod session;
mod shortcut_guide;
mod web_activation;
mod web_search;
#[cfg(windows)]
mod windows_application;
#[cfg(windows)]
mod windows_file;
#[cfg(windows)]
mod windows_web;

pub use action_binding::ActionBinding;
pub use action_binding::ActionBindingError;
pub use action_input::ActionInput;
pub use action_input::ActionInputScalar;
pub use application_activation::ApplicationActivationClient;
pub use application_activation::ApplicationActivationCompletionError;
pub use application_activation::ApplicationActivationQueueCapacity;
pub use application_activation::ApplicationActivationService;
pub use application_activation::ApplicationActivationShutdownError;
pub use application_activation::ApplicationActivationSubmitError;
pub use application_activation::ApplicationActivationTicket;
pub use application_activation::ApplicationLaunchFailure;
pub use application_activation::ApplicationLauncher;
pub use application_catalog::ApplicationCatalog;
pub use application_catalog::ApplicationDescriptor;
pub use application_catalog::ApplicationId;
pub use bound_action::BoundAction;
pub use command_palette::CommandPalette;
pub use command_palette::PaletteAction;
pub use command_palette::PaletteActionState;
pub use command_palette::PaletteLocalResult;
pub use command_palette::PaletteMatches;
pub use command_palette::PaletteQuery;
pub use command_palette::PaletteResults;
pub use command_palette::PaletteSearchTerms;
pub use command_palette::PaletteSelectionMove;
pub use command_palette::WebSearchRequest;
pub use command_palette::WebSearchTerms;
pub use file_activation::FileActivationClient;
pub use file_activation::FileActivationCompletionError;
pub use file_activation::FileActivationFailure;
pub use file_activation::FileActivationQueueCapacity;
pub use file_activation::FileActivationService;
pub use file_activation::FileActivationShutdownError;
pub use file_activation::FileActivationSubmitError;
pub use file_activation::FileActivationTicket;
pub use file_activation::FileLaunchFailure;
pub use file_activation::FileLauncher;
pub use palette_activation::PaletteApplicationInvocation;
pub use palette_activation::PaletteCompletion;
pub use palette_activation::PaletteCompletionDisposition;
pub use palette_activation::PaletteEffect;
pub use palette_activation::PaletteFailure;
pub use palette_activation::PaletteFileInvocation;
pub use palette_activation::PaletteInvocation;
pub use palette_activation::PaletteSubmission;
pub use palette_activation::PaletteWebInvocation;
pub use palette_activation::PendingPaletteInvocation;
pub use palette_controller::PaletteAttemptId;
pub use palette_controller::PaletteContent;
pub use palette_controller::PaletteController;
pub use palette_controller::PaletteSearchStatus;
pub use palette_controller::PaletteStatus;
pub use palette_search::PaletteQueryRevision;
pub use palette_search::PaletteSearch;
pub use palette_search::PaletteSearchBroker;
pub use palette_search::PaletteSearchCompletion;
pub use palette_search::PaletteSearchCompletionDisposition;
pub use palette_search::PaletteSearchFailure;
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
pub use windows_application::ApplicationDiscoveryError;
#[cfg(windows)]
pub use windows_application::WindowsApplicationLauncher;
#[cfg(windows)]
pub use windows_application::discover_installed_applications;
#[cfg(windows)]
pub use windows_file::WindowsFileLauncher;
#[cfg(windows)]
pub use windows_web::WebUriSupport;
#[cfg(windows)]
pub use windows_web::WindowsWebLaunchError;
#[cfg(windows)]
pub use windows_web::WindowsWebLauncher;
