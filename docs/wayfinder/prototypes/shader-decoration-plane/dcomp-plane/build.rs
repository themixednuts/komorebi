use std::{
    env,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context as _, Result, bail};

fn main() -> Result<()> {
    println!("cargo:rerun-if-changed=src/decoration.hlsl");
    println!("cargo:rerun-if-changed=src/decoration.wgsl");
    let fxc = find_windows_sdk_tool("fxc.exe")?;
    let dxc = find_windows_sdk_tool("dxc.exe")?;
    let source = PathBuf::from("src/decoration.hlsl");
    let output = PathBuf::from(env::var_os("OUT_DIR").context("OUT_DIR is set by Cargo")?);
    compile(
        &fxc,
        &source,
        &output.join("decoration.vs.bin"),
        "vs_main",
        "vs_5_0",
    )?;
    compile(
        &fxc,
        &source,
        &output.join("decoration.ps.bin"),
        "ps_main",
        "ps_5_0",
    )?;
    compile(
        &dxc,
        &source,
        &output.join("decoration.dxil.vs.bin"),
        "vs_main",
        "vs_6_0",
    )?;
    compile(
        &dxc,
        &source,
        &output.join("decoration.dxil.ps.bin"),
        "ps_main",
        "ps_6_0",
    )?;

    let generated_hlsl = translate_wgsl(&fs::read_to_string("src/decoration.wgsl")?)?;
    let generated_path = output.join("decoration.naga.hlsl");
    fs::write(&generated_path, generated_hlsl)?;
    compile(
        &dxc,
        &generated_path,
        &output.join("decoration.naga.dxil.ps.bin"),
        "ps_main",
        "ps_6_0",
    )?;
    compile(
        &fxc,
        &generated_path,
        &output.join("decoration.naga.dxbc.ps.bin"),
        "ps_main",
        "ps_5_0",
    )?;
    Ok(())
}

fn translate_wgsl(source: &str) -> Result<String> {
    let module = naga::front::wgsl::parse_str(source).context("parse WGSL")?;
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .context("validate WGSL")?;
    let options = naga::back::hlsl::Options {
        shader_model: naga::back::hlsl::ShaderModel::V5_0,
        ..Default::default()
    };
    let pipeline = naga::back::hlsl::PipelineOptions {
        entry_point: Some((naga::ShaderStage::Fragment, "ps_main".into())),
    };
    let mut hlsl = String::new();
    let mut writer = naga::back::hlsl::Writer::new(&mut hlsl, &options, &pipeline);
    writer
        .write(&module, &info, None)
        .context("translate WGSL to HLSL")?;
    Ok(hlsl)
}

fn compile(compiler: &Path, source: &Path, output: &Path, entry: &str, target: &str) -> Result<()> {
    let result = Command::new(compiler)
        .args([OsStr::new("/nologo"), OsStr::new("/O3"), OsStr::new("/Ges")])
        .arg("/E")
        .arg(entry)
        .arg("/T")
        .arg(target)
        .arg("/Fo")
        .arg(output)
        .arg(source)
        .output()
        .with_context(|| format!("run {}", compiler.display()))?;
    if !result.status.success() {
        // Compiler stderr is human-only diagnostics; it never feeds path identity or shader bytes.
        bail!(
            "shader compiler failed for {entry}/{target}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    Ok(())
}

fn find_windows_sdk_tool(name: &str) -> Result<PathBuf> {
    let program_files = env::var_os("ProgramFiles(x86)").context("ProgramFiles(x86) is set")?;
    let bin = PathBuf::from(program_files)
        .join("Windows Kits")
        .join("10")
        .join("bin");
    let mut versions = fs::read_dir(&bin)
        .with_context(|| format!("read {}", bin.display()))?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    versions.sort_by(|left, right| right.file_name().cmp(&left.file_name()));
    versions
        .into_iter()
        .map(|version| version.join("x64").join(name))
        .find(|candidate| candidate.is_file())
        .with_context(|| format!("find {name} under {}", bin.display()))
}
