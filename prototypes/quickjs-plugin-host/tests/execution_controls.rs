#![allow(clippy::expect_used)]

use std::{fs, time::Duration};

use quickjs_plugin_spike::{
    CancellationFlag, ExecuteError, HostConfig, PluginHost, PluginRequest, Unconfigured,
};

#[tokio::test]
async fn infinite_script_is_interrupted_at_the_host_deadline() {
    let project = tempfile::tempdir().expect("create temporary plugin project");
    let entry = project.path().join("main.ts");
    fs::write(&entry, "while (true) {}\n").expect("write infinite plugin");
    let mut config = HostConfig::for_root(project.path());
    config.timeout = Duration::from_millis(20);
    let host = PluginHost::<Unconfigured>::new()
        .configure(config)
        .expect("configure plugin host");

    let error = host
        .execute(PluginRequest::new(entry))
        .await
        .expect_err("infinite script must time out");

    assert!(matches!(error, ExecuteError::TimedOut { .. }));
}

#[tokio::test]
async fn cancellation_uses_the_same_preemptive_interrupt_path() {
    let project = tempfile::tempdir().expect("create temporary plugin project");
    let entry = project.path().join("main.ts");
    fs::write(&entry, "while (true) {}\n").expect("write infinite plugin");
    let cancellation = CancellationFlag::new();
    cancellation.cancel();
    let host = PluginHost::<Unconfigured>::new()
        .configure(HostConfig::for_root(project.path()))
        .expect("configure plugin host");

    let error = host
        .execute(PluginRequest::new(entry).with_cancellation(cancellation))
        .await
        .expect_err("cancelled script must stop");

    assert!(matches!(error, ExecuteError::Cancelled));
}

#[tokio::test]
async fn allocation_over_the_runtime_budget_is_rejected() {
    let project = tempfile::tempdir().expect("create temporary plugin project");
    let entry = project.path().join("main.ts");
    fs::write(
        &entry,
        "globalThis.data = new Array(1_000_000).fill('xxxxxxxx');\n",
    )
    .expect("write allocating plugin");
    let mut config = HostConfig::for_root(project.path());
    config.memory_limit_bytes = 2 * 1024 * 1024;
    let host = PluginHost::<Unconfigured>::new()
        .configure(config)
        .expect("configure plugin host");

    let error = host
        .execute(PluginRequest::new(entry))
        .await
        .expect_err("allocation must exceed memory budget");

    assert!(error.to_string().contains("out of memory"), "{error}");
}
