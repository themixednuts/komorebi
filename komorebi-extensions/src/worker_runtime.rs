use std::env;
use std::ffi::OsString;
use std::fs::File;
use std::os::windows::io::FromRawHandle;
use std::os::windows::io::RawHandle;

use parking_lot::Mutex;

use crate::PluginLimits;
use crate::PluginLoadFailure;
use crate::PluginLoadReport;
use crate::PluginManifest;
use crate::PluginOutput;
use crate::PluginOutputSink;
use crate::PluginProgram;
use crate::PluginVm;
use crate::run_worker_containment_probe;
use crate::wire;
use crate::wire::Request;
use crate::wire::Response;
use crate::wire::WireError;

const INVALID_ARGUMENTS: i32 = 100;
const WIRE_FAILURE: i32 = 102;
const AMBIENT_ENVIRONMENT: i32 = 103;

/// Runs either the trusted containment probe or the brokered worker loop.
#[doc(hidden)]
pub fn run_extension_worker() -> i32 {
    if let Err(failure) = run_worker_containment_probe() {
        return failure.exit_code();
    }
    let arguments = env::args_os().skip(1).collect::<Vec<_>>();
    if arguments.is_empty() {
        return 0;
    }
    if !broker_environment_is_minimal() {
        return AMBIENT_ENVIRONMENT;
    }
    let Some((reader, writer)) = broker_files(&arguments) else {
        return INVALID_ARGUMENTS;
    };
    run_broker(reader, writer).map_or_else(|_| WIRE_FAILURE, |()| 0)
}

fn broker_environment_is_minimal() -> bool {
    let mut count = 0;
    for (key, value) in env::vars_os() {
        let Some(key) = key.to_str() else {
            return false;
        };
        if value.is_empty()
            || !["LOCALAPPDATA", "SystemRoot", "TEMP", "TMP"]
                .iter()
                .any(|allowed| key.eq_ignore_ascii_case(allowed))
        {
            return false;
        }
        count += 1;
    }
    count == 4
}

fn broker_files(arguments: &[OsString]) -> Option<(File, File)> {
    let [mode, read_handle, write_handle] = arguments else {
        return None;
    };
    if mode != "--broker" {
        return None;
    }
    let read_handle = parse_handle(read_handle)?;
    let write_handle = parse_handle(write_handle)?;
    let reader = unsafe {
        // SAFETY: the launcher transfers unique ownership of this inherited pipe handle.
        File::from_raw_handle(read_handle)
    };
    let writer = unsafe {
        // SAFETY: the launcher transfers unique ownership of this inherited pipe handle.
        File::from_raw_handle(write_handle)
    };
    Some((reader, writer))
}

fn parse_handle(value: &OsString) -> Option<RawHandle> {
    let numeric = value.to_str()?.parse::<usize>().ok()?;
    if numeric == 0 || numeric == usize::MAX {
        return None;
    }
    Some(numeric as RawHandle)
}

fn run_broker(mut reader: File, mut writer: File) -> Result<(), WireError> {
    wire::write_response(&mut writer, &Response::Ready)?;
    let Request::Initialize {
        manifest,
        limits,
        program,
    } = wire::read_request(&mut reader)?
    else {
        return Err(WireError::Invalid("first request must initialize"));
    };

    let mut vm = load_candidate(&mut writer, manifest.clone(), limits, program)?;
    loop {
        match wire::read_request(&mut reader)? {
            Request::Initialize { .. } => {
                return Err(WireError::Invalid("worker already initialized"));
            }
            Request::Reload(program) => {
                if let Some(candidate) =
                    load_candidate(&mut writer, manifest.clone(), limits, program)?
                {
                    vm = Some(candidate);
                }
            }
            Request::Shutdown => {
                wire::write_response(&mut writer, &Response::Stopped)?;
                drop(vm);
                return Ok(());
            }
        }
    }
}

fn load_candidate(
    writer: &mut File,
    manifest: PluginManifest,
    limits: PluginLimits,
    program: PluginProgram,
) -> Result<Option<PluginVm>, WireError> {
    let outputs = RecordingOutputs::default();
    let result = PluginVm::new(manifest, limits, outputs.clone()).and_then(|vm| {
        vm.load(program)?;
        Ok(vm)
    });
    match result {
        Ok(vm) => {
            let report = PluginLoadReport::new(outputs.take());
            wire::write_response(writer, &Response::Loaded(report))?;
            Ok(Some(vm))
        }
        Err(error) => {
            wire::write_response(writer, &Response::Rejected(PluginLoadFailure::from(error)))?;
            Ok(None)
        }
    }
}

#[derive(Clone, Default)]
struct RecordingOutputs(std::sync::Arc<Mutex<Vec<PluginOutput>>>);

impl RecordingOutputs {
    fn take(&self) -> Vec<PluginOutput> {
        std::mem::take(&mut *self.0.lock())
    }
}

impl PluginOutputSink for RecordingOutputs {
    fn emit(&self, output: PluginOutput) {
        self.0.lock().push(output);
    }
}
