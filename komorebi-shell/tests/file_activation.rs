use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use komorebi_search::FileSearchLimit;
use komorebi_search::FileSearchQueueCapacity;
use komorebi_search::FileSearchService;
use komorebi_shell::FileActivationQueueCapacity;
use komorebi_shell::FileActivationService;
use komorebi_shell::FileLaunchFailure;
use komorebi_shell::FileLauncher;

#[derive(Clone, Default)]
struct RecordingLauncher {
    paths: Arc<Mutex<Vec<PathBuf>>>,
}

impl FileLauncher for RecordingLauncher {
    async fn launch(&self, path: PathBuf) -> Result<(), FileLaunchFailure> {
        self.paths
            .lock()
            .map_err(|_| FileLaunchFailure::new("recording launcher lock was poisoned"))?
            .push(path);
        Ok(())
    }
}

#[tokio::test]
async fn broker_owns_admitted_activation_after_ticket_interest_is_dropped()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let exact_path = directory.path().join("owned-activation.txt");
    std::fs::write(&exact_path, b"owned")?;
    let files = FileSearchService::start(
        directory.path().to_path_buf(),
        FileSearchQueueCapacity::new(1).ok_or("one is a valid search capacity")?,
    )
    .await?;
    let result = files
        .client()
        .search(
            "owned activation",
            FileSearchLimit::new(1).ok_or("one is a valid result limit")?,
        )
        .await?
        .pop()
        .ok_or("the indexed file should match")?;
    let launcher = RecordingLauncher::default();
    let activation = FileActivationService::start(
        files.client(),
        launcher.clone(),
        FileActivationQueueCapacity::new(1).ok_or("one is a valid activation capacity")?,
    );

    let abandoned = activation.client().submit(result.id().clone()).await?;
    drop(abandoned);
    activation.shutdown().await?;

    assert_eq!(
        launcher
            .paths
            .lock()
            .map_err(|_| "recording launcher lock was poisoned")?
            .as_slice(),
        [exact_path]
    );
    files.shutdown().await?;
    Ok(())
}

#[cfg(windows)]
#[tokio::test]
async fn windows_launcher_rejects_an_interior_utf16_nul_before_native_activation()
-> Result<(), Box<dyn std::error::Error>> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt as _;

    use komorebi_shell::WindowsFileLauncher;

    let path = PathBuf::from(OsString::from_wide(&[
        u16::from(b'C'),
        u16::from(b':'),
        u16::from(b'\\'),
        u16::from(b'a'),
        0,
        u16::from(b'b'),
    ]));

    let error = WindowsFileLauncher
        .launch(path)
        .await
        .err()
        .ok_or("an interior NUL must never reach ShellExecuteExW")?;
    assert_eq!(error.native_code(), None);
    assert_eq!(error.message(), "file path contains an interior UTF-16 NUL");
    Ok(())
}
