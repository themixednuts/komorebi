use std::sync::Arc;

use anyhow::{Context as _, Result};
use mlua::{Function as LuaFunction, Lua, Table as LuaTable};
use parking_lot::Mutex;
use rquickjs::{CatchResultExt, Context, Function, Module, Object, Runtime};

use crate::{
    Direction,
    module_loader::{ModuleTelemetry, PluginLoader, PluginResolver},
    path_key,
};

use super::{EVENTS, WorkloadProof, fixture_root};

pub(super) fn quickjs() -> Result<WorkloadProof> {
    let root = fixture_root("typescript").canonicalize()?;
    let entry = root.join("plugin.ts").canonicalize()?;
    let actions = Arc::new(Mutex::new(Vec::new()));
    let callback_actions = Arc::clone(&actions);
    let telemetry = Arc::new(ModuleTelemetry::default());
    let runtime = Runtime::new().context("create QuickJS benchmark runtime")?;
    runtime.set_loader(PluginResolver::new(root), PluginLoader::new(telemetry));
    let context = Context::full(&runtime).context("create QuickJS benchmark context")?;

    context.with(|ctx| -> Result<WorkloadProof> {
        let focus = Function::new(ctx.clone(), move |value: String| {
            let direction = match value.as_str() {
                "left" => Direction::Left,
                "right" => Direction::Right,
                "up" => Direction::Up,
                "down" => Direction::Down,
                _ => {
                    return Err(rquickjs::Error::new_from_js_message(
                        "string",
                        "Direction",
                        format!("unknown direction {value:?}"),
                    ));
                }
            };
            callback_actions
                .lock()
                .push(direction_name(&direction).to_owned());
            Ok::<_, rquickjs::Error>(())
        })?;
        ctx.globals().set("__komorebi_focus", focus)?;
        let namespace = Module::import(&ctx, path_key::encode(&entry))
            .catch(&ctx)
            .map_err(|error| anyhow::anyhow!(error.to_string()))?
            .finish::<Object>()
            .catch(&ctx)
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let invoke: Function = namespace.get("invoke")?;
        let snapshot: Function = namespace.get("snapshot")?;
        let mut checksum = 0_i64;
        for index in 0..EVENTS {
            let event = Object::new(ctx.clone())?;
            event.set("windowId", i32::try_from(index + 1)?)?;
            event.set("workspace", i32::try_from(index % 4)?)?;
            checksum += invoke.call::<_, i64>((event,))?;
        }
        let snapshot = snapshot.call::<_, i64>(())?;
        Ok(WorkloadProof {
            checksum,
            actions: actions.lock().clone(),
            snapshot,
        })
    })
}

pub(super) fn lua(jit: bool) -> Result<WorkloadProof> {
    let root = fixture_root("lua");
    let scoring_source = std::fs::read_to_string(root.join("scoring.lua"))?;
    let plugin_source = std::fs::read_to_string(root.join("plugin.lua"))?;
    let lua = Lua::new();
    super::lua::configure_jit(&lua, jit)?;
    let actions = Arc::new(Mutex::new(Vec::new()));
    let callback_actions = Arc::clone(&actions);
    lua.globals().set(
        "focus",
        lua.create_function(move |_, value: String| match value.as_str() {
            "left" | "right" | "up" | "down" => {
                callback_actions.lock().push(value);
                Ok(())
            }
            _ => Err(mlua::Error::external("unknown direction")),
        })?,
    )?;
    let package: LuaTable = lua.globals().get("package")?;
    let preload: LuaTable = package.get("preload")?;
    preload.set(
        "scoring",
        lua.load(scoring_source)
            .set_name("@scoring.lua")
            .into_function()?,
    )?;
    let plugin: LuaTable = lua.load(plugin_source).set_name("@plugin.lua").eval()?;
    let invoke: LuaFunction = plugin.get("invoke")?;
    let snapshot: LuaFunction = plugin.get("snapshot")?;
    let mut checksum = 0_i64;
    for index in 0..EVENTS {
        let event = lua.create_table()?;
        event.set("window_id", i64::try_from(index + 1)?)?;
        event.set("workspace", i64::try_from(index % 4)?)?;
        checksum += invoke.call::<i64>(event)?;
    }
    Ok(WorkloadProof {
        checksum,
        actions: actions.lock().clone(),
        snapshot: snapshot.call(())?,
    })
}

fn direction_name(direction: &Direction) -> &'static str {
    match direction {
        Direction::Left => "left",
        Direction::Right => "right",
        Direction::Up => "up",
        Direction::Down => "down",
    }
}
