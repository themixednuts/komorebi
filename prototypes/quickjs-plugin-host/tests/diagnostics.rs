#![allow(clippy::expect_used)]

use std::fs;

use quickjs_plugin_spike::{HostConfig, PluginHost, PluginRequest, Unconfigured};

#[tokio::test]
async fn runtime_stack_uses_the_original_typescript_path_and_line() {
    let project = tempfile::tempdir().expect("create temporary plugin project");
    let entry = project.path().join("main.ts");
    fs::write(
        &entry,
        "type Marker = string;\n\nconst marker: Marker = 'boom';\nthrow new Error(marker);\n",
    )
    .expect("write failing TypeScript plugin");
    let host = PluginHost::<Unconfigured>::new()
        .configure(HostConfig::for_root(project.path()))
        .expect("configure plugin host");

    let diagnostic = host
        .execute(PluginRequest::new(entry))
        .await
        .expect_err("plugin should throw")
        .to_string();

    assert!(diagnostic.contains("main.ts:4"), "{diagnostic}");
    assert!(!diagnostic.contains("komorebi-file:"), "{diagnostic}");
}
