use std::sync::Arc;
use std::sync::Mutex;

use komorebi_shell::ApplicationActivationQueueCapacity;
use komorebi_shell::ApplicationActivationService;
use komorebi_shell::ApplicationId;
use komorebi_shell::ApplicationLaunchFailure;
use komorebi_shell::ApplicationLauncher;

#[derive(Clone)]
struct RecordingLauncher {
    launched: Arc<Mutex<Vec<ApplicationId>>>,
}

impl ApplicationLauncher for RecordingLauncher {
    async fn launch(&self, id: ApplicationId) -> Result<(), ApplicationLaunchFailure> {
        self.launched
            .lock()
            .map_err(|error| ApplicationLaunchFailure::new(error.to_string()))?
            .push(id);
        Ok(())
    }
}

#[tokio::test]
async fn admitted_application_activation_survives_dropped_result_interest()
-> Result<(), Box<dyn std::error::Error>> {
    let launched = Arc::new(Mutex::new(Vec::new()));
    let service = ApplicationActivationService::start(
        RecordingLauncher {
            launched: Arc::clone(&launched),
        },
        ApplicationActivationQueueCapacity::new(1).ok_or("one is a valid capacity")?,
    );
    let id = ApplicationId::from_utf16(
        "shell:AppsFolder\\example.app"
            .encode_utf16()
            .collect::<Vec<_>>(),
    )
    .ok_or("the test identity is valid")?;

    drop(service.client().submit(id.clone()).await?);
    service.shutdown().await?;

    assert_eq!(*launched.lock().map_err(|error| error.to_string())?, [id]);
    Ok(())
}
