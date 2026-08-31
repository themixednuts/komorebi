use std::error::Error;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::os::windows::ffi::OsStrExt as _;
use std::os::windows::ffi::OsStringExt as _;
use std::path::PathBuf;
use std::time::Duration;

use komorebi_extensions::InstructionBudget;
use komorebi_extensions::MemoryBudget;
use komorebi_extensions::PluginCapability;
use komorebi_extensions::PluginCapabilitySet;
use komorebi_extensions::PluginHostQueueCapacity;
use komorebi_extensions::PluginHotReloadEvent;
use komorebi_extensions::PluginHotReloadEventCapacity;
use komorebi_extensions::PluginHotReloadQuietPeriod;
use komorebi_extensions::PluginHotReloadService;
use komorebi_extensions::PluginHotReloadSettings;
use komorebi_extensions::PluginId;
use komorebi_extensions::PluginLimits;
use komorebi_extensions::PluginLogRecord;
use komorebi_extensions::PluginManifest;
use komorebi_extensions::PluginOutput;
use komorebi_extensions::PluginProgram;
use komorebi_extensions::PluginSourceFile;
use windows_sys::Win32::Storage::FileSystem::MOVEFILE_REPLACE_EXISTING;
use windows_sys::Win32::Storage::FileSystem::MOVEFILE_WRITE_THROUGH;
use windows_sys::Win32::Storage::FileSystem::MoveFileExW;

fn program_source(message: &str) -> String {
    format!("return {{ on_load = function(context) context:info('{message}') end }}")
}

fn only_log(outputs: &[PluginOutput]) -> Result<&PluginLogRecord, Box<dyn Error>> {
    let [PluginOutput::Log(record)] = outputs else {
        return Err("expected exactly one log output".into());
    };
    Ok(record)
}

fn limits() -> Result<PluginLimits, Box<dyn Error>> {
    Ok(PluginLimits::new(
        MemoryBudget::new(2 * 1024 * 1024).ok_or("memory budget must be nonzero")?,
        InstructionBudget::new(50_000).ok_or("instruction budget must be nonzero")?,
    ))
}

fn settings() -> Result<PluginHotReloadSettings, Box<dyn Error>> {
    Ok(PluginHotReloadSettings::new(
        PluginHostQueueCapacity::new(1).ok_or("host capacity must be nonzero")?,
        PluginHotReloadEventCapacity::new(4).ok_or("event capacity must be nonzero")?,
        PluginHotReloadQuietPeriod::new(Duration::from_millis(30))
            .ok_or("quiet period must be nonzero")?,
    ))
}

fn replace_file(source: &std::path::Path, destination: &std::path::Path) -> io::Result<()> {
    let source = source
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    // SAFETY: both path buffers are NUL-terminated and live for the duration of the call.
    let replaced = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if replaced == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[tokio::test]
async fn native_file_event_reloads_exact_source_and_ignores_sibling_changes()
-> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let source_path = directory.path().join("extension.lua");
    let sibling_path = directory.path().join("unrelated.lua");
    fs::write(&source_path, program_source("initial"))?;

    let source = PluginSourceFile::open(&source_path, "extension")?;
    let worker = PathBuf::from(env!("CARGO_BIN_EXE_komorebi-extension-worker"));
    let mut hot_reload = PluginHotReloadService::start(
        worker,
        PluginManifest::new(
            PluginId::parse("hot-reload-test")?,
            PluginCapabilitySet::only([PluginCapability::Log]),
        ),
        limits()?,
        source,
        settings()?,
    )
    .await?;
    assert_eq!(
        only_log(hot_reload.initial_load().outputs())?.message(),
        "initial"
    );

    fs::write(&sibling_path, program_source("unrelated"))?;
    assert!(
        tokio::time::timeout(Duration::from_millis(150), hot_reload.next_event())
            .await
            .is_err()
    );

    fs::write(&source_path, program_source("reloaded"))?;
    let event = tokio::time::timeout(Duration::from_secs(5), hot_reload.next_event())
        .await?
        .ok_or("hot-reload event channel closed")?;
    let PluginHotReloadEvent::Reloaded(report) = event else {
        return Err(format!("expected successful reload, got {event:?}").into());
    };
    assert_eq!(only_log(report.outputs())?.message(), "reloaded");

    hot_reload.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn rejected_source_does_not_end_event_driven_reload_ownership() -> Result<(), Box<dyn Error>>
{
    let directory = tempfile::tempdir()?;
    let source_path = directory.path().join("extension.lua");
    fs::write(&source_path, program_source("initial"))?;
    let source = PluginSourceFile::open(&source_path, "extension")?;
    let worker = PathBuf::from(env!("CARGO_BIN_EXE_komorebi-extension-worker"));
    let mut hot_reload = PluginHotReloadService::start(
        worker,
        PluginManifest::new(
            PluginId::parse("hot-reload-rejection-test")?,
            PluginCapabilitySet::only([PluginCapability::Log]),
        ),
        limits()?,
        source,
        settings()?,
    )
    .await?;

    fs::write(&source_path, "return {}")?;
    let rejected = tokio::time::timeout(Duration::from_secs(5), hot_reload.next_event())
        .await?
        .ok_or("hot-reload event channel closed")?;
    assert!(matches!(rejected, PluginHotReloadEvent::Rejected(_)));

    fs::write(&source_path, program_source("recovered"))?;
    let recovered = tokio::time::timeout(Duration::from_secs(5), hot_reload.next_event())
        .await?
        .ok_or("hot-reload event channel closed")?;
    let PluginHotReloadEvent::Reloaded(report) = recovered else {
        return Err(format!("expected recovered reload, got {recovered:?}").into());
    };
    assert_eq!(only_log(report.outputs())?.message(), "recovered");

    hot_reload.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn atomic_file_replacement_is_observed_without_reregistering_the_watch()
-> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let source_path = directory.path().join("extension.lua");
    let replacement_path = directory.path().join("extension.lua.new");
    fs::write(&source_path, program_source("initial"))?;
    let source = PluginSourceFile::open(&source_path, "extension")?;
    let worker = PathBuf::from(env!("CARGO_BIN_EXE_komorebi-extension-worker"));
    let mut hot_reload = PluginHotReloadService::start(
        worker,
        PluginManifest::new(
            PluginId::parse("atomic-hot-reload-test")?,
            PluginCapabilitySet::only([PluginCapability::Log]),
        ),
        limits()?,
        source,
        settings()?,
    )
    .await?;

    fs::write(&replacement_path, program_source("atomically-replaced"))?;
    replace_file(&replacement_path, &source_path)?;

    let event = tokio::time::timeout(Duration::from_secs(5), hot_reload.next_event())
        .await?
        .ok_or("hot-reload event channel closed")?;
    let PluginHotReloadEvent::Reloaded(report) = event else {
        return Err(format!("expected successful reload, got {event:?}").into());
    };
    assert_eq!(only_log(report.outputs())?.message(), "atomically-replaced");

    hot_reload.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn source_identity_and_loading_preserve_unpaired_utf16_path_units()
-> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let filename = OsString::from_wide(&[
        u16::from(b'e'),
        u16::from(b'x'),
        u16::from(b't'),
        0xd800,
        u16::from(b'.'),
        u16::from(b'l'),
        u16::from(b'u'),
        u16::from(b'a'),
    ]);
    let source_path = directory.path().join(filename);
    fs::write(&source_path, program_source("lossless"))?;

    let source = PluginSourceFile::open(&source_path, "lossless")?;
    let resolved_units = source.path().as_os_str().encode_wide().collect::<Vec<_>>();
    let expected_units = source_path.as_os_str().encode_wide().collect::<Vec<_>>();
    let resolved_units = resolved_units
        .strip_prefix(&[
            u16::from(b'\\'),
            u16::from(b'\\'),
            u16::from(b'?'),
            u16::from(b'\\'),
        ])
        .unwrap_or(&resolved_units);

    assert_eq!(resolved_units, expected_units);
    assert_eq!(
        source.load().await?,
        PluginProgram::new("lossless", program_source("lossless"))?
    );
    Ok(())
}
