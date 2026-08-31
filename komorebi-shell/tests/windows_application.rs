#![cfg(windows)]

use komorebi_shell::discover_installed_applications;

#[tokio::test]
async fn public_apps_folder_produces_launchable_application_identities()
-> Result<(), Box<dyn std::error::Error>> {
    let catalog = discover_installed_applications().await?;
    assert!(!catalog.applications().is_empty());
    assert!(
        catalog
            .applications()
            .iter()
            .all(|application| !application.name().trim().is_empty())
    );
    Ok(())
}
