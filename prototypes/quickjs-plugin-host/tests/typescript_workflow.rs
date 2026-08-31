#![allow(clippy::expect_used)]

use std::fs;

use quickjs_plugin_spike::{
    Direction, HostAction, HostConfig, PluginHost, PluginRequest, Unconfigured,
};

#[tokio::test]
async fn typescript_without_tsconfig_imports_modules_and_awaits_host_calls() {
    let project = tempfile::tempdir().expect("create temporary plugin project");
    fs::write(
        project.path().join("directions.ts"),
        "export type Direction = 'left' | 'right';\nexport const target: Direction = 'left';\n",
    )
    .expect("write imported TypeScript module");
    fs::write(
        project.path().join("main.ts"),
        concat!(
            "import { focus } from 'komorebi:host';\n",
            "import { target, type Direction } from './directions';\n",
            "const chosen: Direction = target;\n",
            "await focus(chosen);\n",
        ),
    )
    .expect("write entry TypeScript module");

    assert!(!project.path().join("tsconfig.json").exists());
    let host = PluginHost::<Unconfigured>::new()
        .configure(HostConfig::for_root(project.path()))
        .expect("configure plugin host");
    let report = host
        .execute(PluginRequest::new(project.path().join("main.ts")))
        .await
        .expect("execute TypeScript plugin");

    assert_eq!(report.actions, [HostAction::Focus(Direction::Left)]);
    assert_eq!(report.transformed_modules, 2);
}
