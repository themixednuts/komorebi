use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=src/particle.hlsl");
    let fxc = find_windows_sdk_tool("fxc.exe");
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set by Cargo"));
    let compiled = Command::new(&fxc)
        .args([
            "/nologo", "/O3", "/Ges", "/E", "cs_main", "/T", "cs_5_0", "/Fo",
        ])
        .arg(output.join("particle.cs.bin"))
        .arg("src/particle.hlsl")
        .output()
        .unwrap_or_else(|error| panic!("run {}: {error}", fxc.display()));
    if !compiled.status.success() {
        panic!("fxc failed: {}", String::from_utf8_lossy(&compiled.stderr));
    }
}

fn find_windows_sdk_tool(name: &str) -> PathBuf {
    let program_files = env::var_os("ProgramFiles(x86)").expect("ProgramFiles(x86) is set");
    let bin = PathBuf::from(program_files)
        .join("Windows Kits")
        .join("10")
        .join("bin");
    let mut versions = fs::read_dir(&bin)
        .unwrap_or_else(|error| panic!("read {}: {error}", bin.display()))
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    versions.sort_by(|left, right| right.file_name().cmp(&left.file_name()));
    versions
        .into_iter()
        .map(|version| version.join("x64").join(name))
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| panic!("find {name} under {}", bin.display()))
}
