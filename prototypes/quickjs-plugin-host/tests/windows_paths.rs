#![allow(clippy::expect_used)]

use std::{ffi::OsString, fs, os::windows::ffi::OsStringExt};

use quickjs_plugin_spike::{
    Direction, HostAction, HostConfig, PluginHost, PluginRequest, Unconfigured,
};

#[tokio::test]
async fn plugin_root_preserves_an_unpaired_utf16_surrogate() {
    let parent = tempfile::tempdir().expect("create temporary parent directory");
    let mut component = "scripts-".encode_utf16().collect::<Vec<_>>();
    component.push(0xd800);
    let root = parent.path().join(OsString::from_wide(&component));
    fs::create_dir(&root).expect("create WTF-16 plugin root");
    fs::write(
        root.join("main.ts"),
        "import { focus } from 'komorebi:host';\nawait focus('left');\n",
    )
    .expect("write plugin in WTF-16 root");

    let host = PluginHost::<Unconfigured>::new()
        .configure(HostConfig::for_root(&root))
        .expect("configure plugin host");
    let report = host
        .execute(PluginRequest::new(root.join("main.ts")))
        .await
        .expect("execute plugin from WTF-16 root");

    assert_eq!(report.actions, [HostAction::Focus(Direction::Left)]);
}
