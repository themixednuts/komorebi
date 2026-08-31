use std::sync::Arc;

use komorebi_shell::WebActivationQueueCapacity;
use komorebi_shell::WebActivationService;
use komorebi_shell::WebActivationSubmitError;
use komorebi_shell::WebLaunchDisposition;
use komorebi_shell::WebLaunchFailure;
use komorebi_shell::WebSearchEndpoint;
use komorebi_shell::WebSearchRequest;
use komorebi_shell::WebSearchTarget;
use komorebi_shell::WebUriLauncher;
use tokio::sync::Mutex;
use tokio::sync::Notify;

#[derive(Clone, Default)]
struct ControlledLauncher {
    started: Arc<Notify>,
    release: Arc<Notify>,
    targets: Arc<Mutex<Vec<String>>>,
}

impl WebUriLauncher for ControlledLauncher {
    async fn launch(
        &self,
        target: WebSearchTarget,
    ) -> Result<WebLaunchDisposition, WebLaunchFailure> {
        let started = Arc::clone(&self.started);
        let release = Arc::clone(&self.release);
        let targets = Arc::clone(&self.targets);
        targets.lock().await.push(target.as_str().to_owned());
        started.notify_one();
        release.notified().await;
        Ok(WebLaunchDisposition::Launched)
    }
}

#[tokio::test]
async fn broker_owns_admitted_launch_after_ticket_interest_is_dropped()
-> Result<(), Box<dyn std::error::Error>> {
    let endpoint = WebSearchEndpoint::new("https://search.example/results", "q")?;
    let launcher = ControlledLauncher::default();
    let service = WebActivationService::start(
        endpoint,
        launcher.clone(),
        WebActivationQueueCapacity::new(1).ok_or("one is a valid capacity")?,
    );
    let client = service.client();

    let abandoned = client
        .submit(WebSearchRequest::new("first").ok_or("terms should be nonempty")?)
        .await?;
    launcher.started.notified().await;
    drop(abandoned);
    launcher.release.notify_one();

    let observed = client
        .submit(WebSearchRequest::new("second").ok_or("terms should be nonempty")?)
        .await?;
    launcher.started.notified().await;
    launcher.release.notify_one();
    assert_eq!(
        observed.complete().await?,
        Ok(WebLaunchDisposition::Launched)
    );
    assert_eq!(
        launcher.targets.lock().await.as_slice(),
        [
            "https://search.example/results?q=first",
            "https://search.example/results?q=second"
        ]
    );

    service.shutdown().await?;
    assert!(matches!(
        client
            .submit(WebSearchRequest::new("third").ok_or("terms should be nonempty")?)
            .await,
        Err(WebActivationSubmitError::Stopped)
    ));
    Ok(())
}
