mod decoder;

use std::io;
use std::io::Read;
use std::io::Write;

use thiserror::Error;

use crate::InstructionBudget;
use crate::MemoryBudget;
use crate::PluginCapabilitySet;
use crate::PluginId;
use crate::PluginLimits;
use crate::PluginLoadFailure;
use crate::PluginLoadReport;
use crate::PluginLogLevel;
use crate::PluginLogRecord;
use crate::PluginManifest;
use crate::PluginProgram;
use crate::host_domain::MAX_PLUGIN_LOG_RECORDS;

use self::decoder::Decoder;

const MAGIC: [u8; 4] = *b"KEXT";
const VERSION: u16 = 1;
const MAX_FRAME_BYTES: usize = 2 * 1024 * 1024;

pub(crate) enum Request {
    Initialize {
        manifest: PluginManifest,
        limits: PluginLimits,
        program: PluginProgram,
    },
    Reload(PluginProgram),
    Shutdown,
}

pub(crate) enum Response {
    Ready,
    Loaded(PluginLoadReport),
    Rejected(PluginLoadFailure),
    Stopped,
}

#[derive(Debug, Error)]
pub enum WireError {
    #[error("extension wire I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("extension wire frame exceeds its fixed bound")]
    FrameTooLarge,
    #[error("extension wire frame is invalid: {0}")]
    Invalid(&'static str),
}

pub(crate) fn write_request(writer: &mut impl Write, request: &Request) -> Result<(), WireError> {
    let mut body = header();
    match request {
        Request::Initialize {
            manifest,
            limits,
            program,
        } => {
            body.push(1);
            put_string(&mut body, manifest.id().as_str())?;
            put_u32(&mut body, manifest.capabilities().bits());
            put_u64(
                &mut body,
                u64::try_from(limits.memory().bytes()).map_err(|_| WireError::FrameTooLarge)?,
            );
            put_u64(&mut body, limits.instructions().instructions());
            put_program(&mut body, program)?;
        }
        Request::Reload(program) => {
            body.push(2);
            put_program(&mut body, program)?;
        }
        Request::Shutdown => body.push(3),
    }
    write_frame(writer, &body)
}

pub(crate) fn read_request(reader: &mut impl Read) -> Result<Request, WireError> {
    let body = read_frame(reader)?;
    let mut decoder = Decoder::new(&body);
    decoder.require_header()?;
    let request = match decoder.u8()? {
        1 => {
            let id =
                PluginId::parse(decoder.string()?).map_err(|_| WireError::Invalid("plugin id"))?;
            let capabilities = PluginCapabilitySet::from_bits(decoder.u32()?)
                .ok_or(WireError::Invalid("capability set"))?;
            let memory = usize::try_from(decoder.u64()?)
                .ok()
                .and_then(MemoryBudget::new)
                .ok_or(WireError::Invalid("memory budget"))?;
            let instructions = InstructionBudget::new(decoder.u64()?)
                .ok_or(WireError::Invalid("instruction budget"))?;
            let program = decoder.program()?;
            Request::Initialize {
                manifest: PluginManifest::new(id, capabilities),
                limits: PluginLimits::new(memory, instructions),
                program,
            }
        }
        2 => Request::Reload(decoder.program()?),
        3 => Request::Shutdown,
        _ => return Err(WireError::Invalid("request tag")),
    };
    decoder.finish()?;
    Ok(request)
}

pub(crate) fn write_response(
    writer: &mut impl Write,
    response: &Response,
) -> Result<(), WireError> {
    let mut body = header();
    match response {
        Response::Ready => body.push(0),
        Response::Loaded(report) => {
            body.push(1);
            let count = u16::try_from(report.logs().len()).map_err(|_| WireError::FrameTooLarge)?;
            put_u16(&mut body, count);
            for log in report.logs() {
                body.push(log.level().code());
                put_string(&mut body, log.message())?;
            }
        }
        Response::Rejected(failure) => {
            body.push(2);
            put_failure(&mut body, failure)?;
        }
        Response::Stopped => body.push(3),
    }
    write_frame(writer, &body)
}

pub(crate) fn read_response(
    reader: &mut impl Read,
    plugin: &PluginId,
) -> Result<Response, WireError> {
    let body = read_frame(reader)?;
    let mut decoder = Decoder::new(&body);
    decoder.require_header()?;
    let response = match decoder.u8()? {
        0 => Response::Ready,
        1 => {
            let count = usize::from(decoder.u16()?);
            if count > MAX_PLUGIN_LOG_RECORDS {
                return Err(WireError::Invalid("log count"));
            }
            let mut logs = Vec::with_capacity(count);
            for _ in 0..count {
                let level = PluginLogLevel::from_code(decoder.u8()?)
                    .ok_or(WireError::Invalid("log level"))?;
                let message = decoder.string()?.to_owned().into_boxed_str();
                logs.push(PluginLogRecord::new(plugin.clone(), level, message));
            }
            Response::Loaded(PluginLoadReport::new(logs))
        }
        2 => Response::Rejected(decoder.failure()?),
        3 => Response::Stopped,
        _ => return Err(WireError::Invalid("response tag")),
    };
    decoder.finish()?;
    Ok(response)
}

fn header() -> Vec<u8> {
    let mut body = Vec::with_capacity(128);
    body.extend_from_slice(&MAGIC);
    put_u16(&mut body, VERSION);
    body
}

fn put_program(body: &mut Vec<u8>, program: &PluginProgram) -> Result<(), WireError> {
    let (name, source) = program.as_parts();
    put_string(body, name)?;
    put_bytes(body, source)
}

fn put_failure(body: &mut Vec<u8>, failure: &PluginLoadFailure) -> Result<(), WireError> {
    match failure {
        PluginLoadFailure::CapabilityDenied(capability) => {
            body.extend_from_slice(&[0, capability.code()]);
        }
        PluginLoadFailure::InstructionBudgetExhausted => body.push(1),
        PluginLoadFailure::MemoryBudgetExceeded => body.push(2),
        PluginLoadFailure::MemoryLimitOverflow => body.push(3),
        PluginLoadFailure::MemoryLimitUnavailable => body.push(4),
        PluginLoadFailure::MissingOnLoad => body.push(5),
        PluginLoadFailure::Lua(message) => {
            body.push(6);
            put_string(body, message)?;
        }
        PluginLoadFailure::LogMessageTooLarge => body.push(7),
        PluginLoadFailure::LogBudgetExceeded => body.push(8),
    }
    Ok(())
}

fn put_string(body: &mut Vec<u8>, value: &str) -> Result<(), WireError> {
    put_bytes(body, value.as_bytes())
}

fn put_bytes(body: &mut Vec<u8>, value: &[u8]) -> Result<(), WireError> {
    let length = u32::try_from(value.len()).map_err(|_| WireError::FrameTooLarge)?;
    put_u32(body, length);
    body.extend_from_slice(value);
    if body.len() > MAX_FRAME_BYTES {
        return Err(WireError::FrameTooLarge);
    }
    Ok(())
}

fn put_u16(body: &mut Vec<u8>, value: u16) {
    body.extend_from_slice(&value.to_le_bytes());
}

fn put_u32(body: &mut Vec<u8>, value: u32) {
    body.extend_from_slice(&value.to_le_bytes());
}

fn put_u64(body: &mut Vec<u8>, value: u64) {
    body.extend_from_slice(&value.to_le_bytes());
}

fn write_frame(writer: &mut impl Write, body: &[u8]) -> Result<(), WireError> {
    if body.len() > MAX_FRAME_BYTES {
        return Err(WireError::FrameTooLarge);
    }
    let length = u32::try_from(body.len()).map_err(|_| WireError::FrameTooLarge)?;
    writer.write_all(&length.to_le_bytes())?;
    writer.write_all(body)?;
    writer.flush()?;
    Ok(())
}

fn read_frame(reader: &mut impl Read) -> Result<Vec<u8>, WireError> {
    let mut length = [0; 4];
    reader.read_exact(&mut length)?;
    let length =
        usize::try_from(u32::from_le_bytes(length)).map_err(|_| WireError::FrameTooLarge)?;
    if length > MAX_FRAME_BYTES {
        return Err(WireError::FrameTooLarge);
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_rejects_an_unknown_capability_bit() -> Result<(), Box<dyn std::error::Error>> {
        let mut frame = header();
        frame.push(1);
        put_string(&mut frame, "wire-test")?;
        put_u32(&mut frame, 1 << 31);
        put_u64(&mut frame, 1);
        put_u64(&mut frame, 1);
        put_string(&mut frame, "test")?;
        put_bytes(&mut frame, b"return {}")?;
        let mut encoded = Vec::new();
        write_frame(&mut encoded, &frame)?;

        assert!(matches!(
            read_request(&mut encoded.as_slice()),
            Err(WireError::Invalid("capability set"))
        ));
        Ok(())
    }
}
