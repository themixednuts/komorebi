#![windows_subsystem = "windows"]

use komorebi_protocol::RoleHint;
use komorebi_search::FileSearchLimit;
use komorebi_search::FileSearchQueueCapacity;
use komorebi_search::FileSearchService;
use komorebi_settings::SettingsStore;
use komorebi_shell::CommandPalette;
use komorebi_shell::FileActivationQueueCapacity;
use komorebi_shell::FileActivationService;
use komorebi_shell::PaletteFileSearchBroker;
use komorebi_shell::SessionLifetime;
use komorebi_shell::ShellSession;
use komorebi_shell::WebActivationQueueCapacity;
use komorebi_shell::WebActivationService;
use komorebi_shell::WebSearchBroker;
use komorebi_shell::WindowsFileLauncher;
use komorebi_shell::WindowsWebLauncher;

mod palette_window;

const WEB_ACTIVATION_CAPACITY: usize = 1;
const FILE_SEARCH_CAPACITY: usize = 4;
const FILE_RESULT_LIMIT: usize = 32;
const FILE_ACTIVATION_CAPACITY: usize = 1;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let data_directory = dirs::data_local_dir()
        .ok_or_else(|| std::io::Error::other("Windows local data directory is unavailable"))?
        .join("komorebi");
    std::fs::create_dir_all(&data_directory)?;
    let mut settings = SettingsStore::open(&data_directory.join("settings.sqlite"))?;
    let web_capacity = WebActivationQueueCapacity::new(WEB_ACTIVATION_CAPACITY)
        .ok_or_else(|| std::io::Error::other("web activation capacity is invalid"))?;
    let web_service = settings
        .web_search()?
        .map(|endpoint| WebActivationService::start(endpoint, WindowsWebLauncher, web_capacity));
    let web = web_service
        .as_ref()
        .map_or(WebSearchBroker::Unconfigured, |service| {
            WebSearchBroker::Configured(service.client())
        });
    let file_root = if let Some(root) = settings.file_search_root()? {
        root
    } else {
        let root = dirs::home_dir()
            .ok_or_else(|| std::io::Error::other("Windows user home is unavailable"))?;
        settings.set_file_search_root(&root)?;
        root
    };
    let file_search_capacity = FileSearchQueueCapacity::new(FILE_SEARCH_CAPACITY)
        .ok_or_else(|| std::io::Error::other("file-search capacity is invalid"))?;
    let file_result_limit = FileSearchLimit::new(FILE_RESULT_LIMIT)
        .ok_or_else(|| std::io::Error::other("file-result limit is invalid"))?;
    let file_activation_capacity = FileActivationQueueCapacity::new(FILE_ACTIVATION_CAPACITY)
        .ok_or_else(|| std::io::Error::other("file-activation capacity is invalid"))?;
    let file_service = FileSearchService::start(file_root, file_search_capacity).await?;
    let file_search = PaletteFileSearchBroker::configured(file_service.client(), file_result_limit);
    let file_activation_service = FileActivationService::start(
        file_service.client(),
        WindowsFileLauncher,
        file_activation_capacity,
    );
    let session = ShellSession::start(RoleHint::OwnerControl, SessionLifetime::Persistent)?;
    let catalog = session.handle().catalog_snapshot()?.snapshot().await?;
    let palette = CommandPalette::project(&catalog);
    palette_window::run_palette(
        palette,
        session.handle(),
        web,
        file_search,
        file_activation_service.client(),
    );
    file_activation_service.shutdown().await?;
    file_service.shutdown().await?;
    if let Some(web_service) = web_service {
        web_service.shutdown().await?;
    }
    session.shutdown().await?;
    Ok(())
}
