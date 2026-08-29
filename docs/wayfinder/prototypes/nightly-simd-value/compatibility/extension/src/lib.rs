use anyhow::{Result, anyhow};
use mlua::{Lua, LuaOptions, StdLib, prelude::LuaChunkMode};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ExtensionCompatibility {
    pub luajit_marker: String,
    pub memory_limit_bytes: usize,
}

pub fn run() -> Result<ExtensionCompatibility> {
    let memory_limit_bytes = 16 * 1024 * 1024;
    let lua = Lua::new_with(StdLib::NONE, LuaOptions::default())
        .map_err(|error| anyhow!("create LuaJIT: {error}"))?;
    lua.set_memory_limit(memory_limit_bytes)
        .map_err(|error| anyhow!("bound LuaJIT memory: {error}"))?;
    let marker: String = lua
        .load("return 'luajit-typed-effect-host'")
        .set_mode(LuaChunkMode::Text)
        .eval()
        .map_err(|error| anyhow!("evaluate text-only LuaJIT chunk: {error}"))?;
    Ok(ExtensionCompatibility {
        luajit_marker: marker,
        memory_limit_bytes,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn vendored_luajit_compiles_and_runs_in_its_process_crate()
    -> Result<(), Box<dyn std::error::Error>> {
        let report = super::run()?;
        assert_eq!(report.luajit_marker, "luajit-typed-effect-host");
        assert_eq!(report.memory_limit_bytes, 16 * 1024 * 1024);
        Ok(())
    }
}
