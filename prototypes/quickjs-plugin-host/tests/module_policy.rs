#![allow(clippy::expect_used)]

use std::fs;

use quickjs_plugin_spike::{
    Direction, HostAction, HostConfig, PluginHost, PluginRequest, Unconfigured,
};

#[tokio::test]
async fn dynamic_relative_typescript_import_uses_the_host_resolver() {
    let project = tempfile::tempdir().expect("create temporary plugin project");
    fs::write(
        project.path().join("direction.ts"),
        "export const direction: 'right' = 'right';\n",
    )
    .expect("write dynamically imported module");
    fs::write(
        project.path().join("main.ts"),
        concat!(
            "import { focus } from 'komorebi:host';\n",
            "const module = await import('./direction');\n",
            "await focus(module.direction);\n",
        ),
    )
    .expect("write entry module");
    let host = PluginHost::<Unconfigured>::new()
        .configure(HostConfig::for_root(project.path()))
        .expect("configure plugin host");

    let report = host
        .execute(PluginRequest::new(project.path().join("main.ts")))
        .await
        .expect("execute dynamic import");

    assert_eq!(report.actions, [HostAction::Focus(Direction::Right)]);
}

#[tokio::test]
async fn bare_package_imports_are_denied_by_default() {
    let project = tempfile::tempdir().expect("create temporary plugin project");
    fs::write(
        project.path().join("main.ts"),
        "import value from 'left-pad';\nvoid value;\n",
    )
    .expect("write package import");
    let host = PluginHost::<Unconfigured>::new()
        .configure(HostConfig::for_root(project.path()))
        .expect("configure plugin host");

    let error = host
        .execute(PluginRequest::new(project.path().join("main.ts")))
        .await
        .expect_err("bare package import must be denied");

    assert!(error.to_string().contains("resolving module"), "{error}");
}
