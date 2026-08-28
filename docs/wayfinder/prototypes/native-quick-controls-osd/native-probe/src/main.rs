mod audio;
mod monitor;
mod native_text;
mod osd;
mod platform;
mod winrt;

use anyhow::{Context, Result};
use serde::Serialize;
use windows::Win32::System::WinRT::{RO_INIT_SINGLETHREADED, RoInitialize, RoUninitialize};

struct RuntimeApartment;

impl RuntimeApartment {
    fn initialize() -> windows::core::Result<Self> {
        // SAFETY: this is the first apartment initialization on the process's main thread.
        unsafe { RoInitialize(RO_INIT_SINGLETHREADED) }?;
        Ok(Self)
    }
}

impl Drop for RuntimeApartment {
    fn drop(&mut self) {
        // SAFETY: balances the successful RoInitialize call on this same thread.
        unsafe { RoUninitialize() };
    }
}

#[derive(Debug, Serialize)]
struct ProbeReport {
    machine: platform::MachineObservation,
    audio: audio::AudioProbe,
    monitors: Vec<monitor::MonitorProbe>,
    winrt: winrt::WinRtProbe,
}

fn main() -> Result<()> {
    let _apartment = RuntimeApartment::initialize().context("initialize Windows apartment")?;
    let machine = platform::observe_machine().context("observe machine")?;
    let monitors = monitor::observe().context("observe physical monitors")?;
    let winrt = winrt::observe();
    let audio = audio::measure().context("measure audio and OSD routes")?;
    let report = ProbeReport {
        machine,
        audio,
        monitors,
        winrt,
    };

    println!(
        "{}",
        serde_json::to_string_pretty(&report).context("serialize probe report")?
    );
    Ok(())
}
