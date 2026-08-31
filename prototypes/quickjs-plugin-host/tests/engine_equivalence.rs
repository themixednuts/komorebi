#![allow(clippy::expect_used)]

use quickjs_plugin_spike::{BenchmarkEngine, run_workload_proof};

#[test]
fn quickjs_and_luajit_modes_execute_the_same_plugin_behavior() {
    for engine in [
        BenchmarkEngine::QuickJs,
        BenchmarkEngine::LuaJitOff,
        BenchmarkEngine::LuaJitOn,
    ] {
        let proof = run_workload_proof(engine).expect("execute benchmark fixture");
        assert_eq!(proof.checksum, 57_053, "{engine:?}");
        assert_eq!(
            proof.actions,
            ["left", "right", "left", "right", "left"],
            "{engine:?}"
        );
        assert_eq!(proof.snapshot, 10, "{engine:?}");
    }
}
