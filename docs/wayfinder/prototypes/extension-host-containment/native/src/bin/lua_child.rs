#![windows_subsystem = "windows"]

use anyhow::{Result, anyhow};
use mlua::{ChunkMode, Lua, LuaOptions, StdLib};
use wayfinder_extension_containment_prototype::child;
use wayfinder_extension_containment_prototype::protocol::RuntimeKind;

fn run() -> Result<()> {
    let lua = Lua::new_with(StdLib::NONE, LuaOptions::default())
        .map_err(|error| anyhow!("create restricted LuaJIT: {error}"))?;
    lua.set_memory_limit(64 * 1024 * 1024)
        .map_err(|error| anyhow!("set LuaJIT memory limit: {error}"))?;
    let marker: String = lua
        .load("return 'text-only-luajit'")
        .set_mode(ChunkMode::Text)
        .eval()
        .map_err(|error| anyhow!("run text-only LuaJIT chunk: {error}"))?;
    anyhow::ensure!(marker == "text-only-luajit", "unexpected LuaJIT marker");
    child::run(RuntimeKind::LuaJit)
}

fn main() {
    std::panic::set_hook(Box::new(|info| {
        if let Some(path) = std::env::var_os("KOMOREBI_PROTOTYPE_ERROR_FILE") {
            // The child has no console; this trace is secondary to its process exit code.
            let _ = std::fs::write(path, format!("panic: {info}"));
        }
    }));
    if let Err(error) = run() {
        if let Some(path) = std::env::var_os("KOMOREBI_PROTOTYPE_ERROR_FILE") {
            // The child has no console; this trace is secondary to its process exit code.
            let _ = std::fs::write(path, format!("{error:#}"));
        }
        std::process::exit(1);
    }
}
