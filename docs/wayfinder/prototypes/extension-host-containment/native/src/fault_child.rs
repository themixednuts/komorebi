use std::fs::File;
use std::hint::black_box;
use std::ptr::null;
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use mlua::{ChunkMode, Lua};
use uuid::Uuid;
use windows_sys::Win32::System::Threading::{CreateEventW, INFINITE, WaitForSingleObject};

use crate::child_pipe;
use crate::protocol::{ChildFrame, FaultScenario, FrameCodec, FrameLimit, HostFrame, RuntimeKind};
use crate::windows::{OwnedHandle, current_child_facts, harden_dll_search};

/// Authenticates the fault child, then executes exactly one configured containment fault.
///
/// # Errors
///
/// Returns an error when the typed scenario or authenticated pipe bootstrap is invalid.
pub fn run() -> Result<()> {
    let scenario: FaultScenario =
        serde_json::from_str(&required_env("KOMOREBI_PROTOTYPE_FAULT_SCENARIO")?)?;
    let allocation_chunk =
        required_env("KOMOREBI_PROTOTYPE_ALLOCATION_CHUNK_BYTES")?.parse::<usize>()?;
    let codec = FrameCodec::new(FrameLimit::new(
        required_env("KOMOREBI_PROTOTYPE_FRAME_LIMIT")?.parse::<usize>()?,
    )?);
    let timeout = Duration::from_millis(u64::from(
        required_env("KOMOREBI_PROTOTYPE_PIPE_TIMEOUT_MS")?.parse::<u32>()?,
    ));
    let pipe_path = required_env("KOMOREBI_PROTOTYPE_PIPE")?;
    let mut pipe = child_pipe::open(&pipe_path, timeout)?;
    codec.write(
        &mut pipe,
        &ChildFrame::Hello {
            nonce: required_env("KOMOREBI_PROTOTYPE_NONCE")?.parse::<Uuid>()?,
            runtime: RuntimeKind::Rust,
            facts: current_child_facts(harden_dll_search())?,
        },
    )?;
    let HostFrame::Welcome { generation } = codec.read(&mut pipe)? else {
        bail!("host did not authenticate fault child");
    };
    let HostFrame::RunFault {
        generation: requested,
    } = codec.read(&mut pipe)?
    else {
        bail!("host did not trigger fault child");
    };
    if requested != generation {
        bail!("host triggered the wrong fault generation");
    }
    execute(scenario, allocation_chunk, pipe)
}

fn execute(scenario: FaultScenario, allocation_chunk: usize, pipe: File) -> Result<()> {
    match scenario {
        FaultScenario::CpuLoop => cpu_loop(),
        FaultScenario::AllocationPressure => allocation_pressure(allocation_chunk),
        FaultScenario::Deadlock => deadlock(),
        FaultScenario::IndefiniteWait | FaultScenario::PipeStall => wait_forever(pipe),
        FaultScenario::Disconnect => Ok(()),
        FaultScenario::LuaJitNativeCrash => lua_jit_native_crash(),
    }
}

fn cpu_loop() -> ! {
    loop {
        black_box(1_u64.wrapping_add(1));
    }
}

fn allocation_pressure(chunk_bytes: usize) -> ! {
    let mut allocations = Vec::new();
    loop {
        let mut chunk = vec![0_u8; chunk_bytes];
        chunk.fill(1);
        allocations.push(chunk);
        black_box(&allocations);
    }
}

fn deadlock() -> ! {
    let mutex = Mutex::new(());
    let _first = mutex.lock();
    let _second = mutex.lock();
    unreachable!("non-recursive mutex unexpectedly reacquired")
}

fn wait_forever(_pipe: File) -> Result<()> {
    // SAFETY: null security/name and auto-reset initial-unsignaled mode are valid.
    let event = OwnedHandle::new(unsafe { CreateEventW(null(), 0, 0, null()) })?;
    // SAFETY: event remains valid and intentionally unsignaled for this containment fault.
    unsafe { WaitForSingleObject(event.raw(), INFINITE) };
    Ok(())
}

fn lua_jit_native_crash() -> Result<()> {
    let lua = Lua::new();
    let native_crash = lua
        .create_function(|_, ()| -> mlua::Result<()> { std::process::abort() })
        .map_err(|error| anyhow::anyhow!("create LuaJIT native crash callback: {error}"))?;
    lua.globals()
        .set("native_crash", native_crash)
        .map_err(|error| anyhow::anyhow!("install LuaJIT native crash callback: {error}"))?;
    lua.load("native_crash()")
        .set_mode(ChunkMode::Text)
        .exec()
        .map_err(|error| anyhow::anyhow!("execute LuaJIT native crash: {error}"))
}

fn required_env(key: &str) -> Result<String> {
    std::env::var(key).with_context(|| format!("missing {key}"))
}
