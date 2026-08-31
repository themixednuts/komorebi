#![cfg(windows)]

use std::error::Error;
use std::path::PathBuf;

use komorebi_extensions::InstructionBudget;
use komorebi_extensions::MemoryBudget;
use komorebi_extensions::PluginCapability;
use komorebi_extensions::PluginCapabilitySet;
use komorebi_extensions::PluginHostQueueCapacity;
use komorebi_extensions::PluginHostService;
use komorebi_extensions::PluginId;
use komorebi_extensions::PluginLimits;
use komorebi_extensions::PluginLoadFailure;
use komorebi_extensions::PluginLogLevel;
use komorebi_extensions::PluginManifest;
use komorebi_extensions::PluginProgram;

fn manifest(id: &str) -> Result<PluginManifest, Box<dyn Error>> {
    Ok(PluginManifest::new(
        PluginId::parse(id)?,
        PluginCapabilitySet::only([PluginCapability::Log]),
    ))
}

fn limits() -> Result<PluginLimits, Box<dyn Error>> {
    let memory = MemoryBudget::new(2 * 1024 * 1024).ok_or("memory budget must be nonzero")?;
    let instructions =
        InstructionBudget::new(100_000).ok_or("instruction budget must be nonzero")?;
    Ok(PluginLimits::new(memory, instructions))
}

fn program(name: &str, message: &str) -> Result<PluginProgram, Box<dyn Error>> {
    PluginProgram::new(
        name,
        format!("return {{ on_load = function(ctx) ctx:info('{message}') end }}"),
    )
    .map_err(Into::into)
}

#[tokio::test]
async fn lpac_host_loads_and_transactionally_reloads_a_plugin() -> Result<(), Box<dyn Error>> {
    let worker = PathBuf::from(env!("CARGO_BIN_EXE_komorebi-extension-worker"));
    let capacity = PluginHostQueueCapacity::new(2).ok_or("queue capacity must be nonzero")?;
    let host = PluginHostService::start(
        worker,
        manifest("broker-reload-test")?,
        limits()?,
        program("initial", "first")?,
        capacity,
    )
    .await?;

    assert_eq!(host.initial_load().logs().len(), 1);
    assert_eq!(host.initial_load().logs()[0].level(), PluginLogLevel::Info);
    assert_eq!(host.initial_load().logs()[0].message(), "first");

    let report = host.client().reload(program("reload", "second")?).await?;
    assert_eq!(report.logs().len(), 1);
    assert_eq!(report.logs()[0].message(), "second");

    let rejected = host
        .client()
        .reload(PluginProgram::new("invalid", "return {}")?)
        .await;
    assert!(matches!(
        rejected,
        Err(komorebi_extensions::PluginReloadError::Rejected(
            PluginLoadFailure::MissingOnLoad
        ))
    ));
    let recovered = host
        .client()
        .reload(program("after-rejection", "last-good-worker")?)
        .await?;
    assert_eq!(recovered.logs()[0].message(), "last-good-worker");

    host.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn dropping_reload_interest_does_not_stop_the_owned_worker() -> Result<(), Box<dyn Error>> {
    let worker = PathBuf::from(env!("CARGO_BIN_EXE_komorebi-extension-worker"));
    let capacity = PluginHostQueueCapacity::new(1).ok_or("queue capacity must be nonzero")?;
    let host = PluginHostService::start(
        worker,
        manifest("broker-cancel-test")?,
        limits()?,
        program("initial", "first")?,
        capacity,
    )
    .await?;
    let client = host.client();
    let cancelled_client = client.clone();
    let cancelled_program = program("cancelled", "unobserved")?;
    let task = tokio::spawn(async move { cancelled_client.reload(cancelled_program).await });
    task.abort();
    let _ = task.await;

    let report = client
        .reload(program("after-cancel", "still-alive")?)
        .await?;
    assert_eq!(report.logs()[0].message(), "still-alive");

    host.shutdown().await?;
    Ok(())
}
