use crate::PluginCapability;
use crate::PluginLoadFailure;
use crate::PluginProgram;

use super::MAGIC;
use super::VERSION;
use super::WireError;

pub(super) struct Decoder<'frame> {
    remaining: &'frame [u8],
}

impl<'frame> Decoder<'frame> {
    pub(super) const fn new(body: &'frame [u8]) -> Self {
        Self { remaining: body }
    }

    pub(super) fn require_header(&mut self) -> Result<(), WireError> {
        if self.take(MAGIC.len())? != MAGIC {
            return Err(WireError::Invalid("magic"));
        }
        if self.u16()? != VERSION {
            return Err(WireError::Invalid("version"));
        }
        Ok(())
    }

    pub(super) fn finish(self) -> Result<(), WireError> {
        if self.remaining.is_empty() {
            Ok(())
        } else {
            Err(WireError::Invalid("trailing bytes"))
        }
    }

    pub(super) fn u8(&mut self) -> Result<u8, WireError> {
        Ok(self.take(1)?[0])
    }

    pub(super) fn u16(&mut self) -> Result<u16, WireError> {
        let bytes = self.take(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    pub(super) fn u32(&mut self) -> Result<u32, WireError> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    pub(super) fn u64(&mut self) -> Result<u64, WireError> {
        let bytes = self.take(8)?;
        Ok(u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    pub(super) fn string(&mut self) -> Result<&'frame str, WireError> {
        std::str::from_utf8(self.bytes()?).map_err(|_| WireError::Invalid("UTF-8 string"))
    }

    pub(super) fn program(&mut self) -> Result<PluginProgram, WireError> {
        let name = self.string()?;
        let source = self.bytes()?;
        PluginProgram::new(name, source).map_err(|_| WireError::Invalid("plugin program"))
    }

    pub(super) fn failure(&mut self) -> Result<PluginLoadFailure, WireError> {
        match self.u8()? {
            0 => PluginCapability::from_code(self.u8()?)
                .map(PluginLoadFailure::CapabilityDenied)
                .ok_or(WireError::Invalid("capability failure")),
            1 => Ok(PluginLoadFailure::InstructionBudgetExhausted),
            2 => Ok(PluginLoadFailure::MemoryBudgetExceeded),
            3 => Ok(PluginLoadFailure::MemoryLimitOverflow),
            4 => Ok(PluginLoadFailure::MemoryLimitUnavailable),
            5 => Ok(PluginLoadFailure::MissingOnLoad),
            6 => Ok(PluginLoadFailure::Lua(
                self.string()?.to_owned().into_boxed_str(),
            )),
            7 => Ok(PluginLoadFailure::LogMessageTooLarge),
            8 => Ok(PluginLoadFailure::LogBudgetExceeded),
            _ => Err(WireError::Invalid("load failure")),
        }
    }

    fn bytes(&mut self) -> Result<&'frame [u8], WireError> {
        let length = usize::try_from(self.u32()?).map_err(|_| WireError::FrameTooLarge)?;
        self.take(length)
    }

    fn take(&mut self, length: usize) -> Result<&'frame [u8], WireError> {
        if self.remaining.len() < length {
            return Err(WireError::Invalid("truncated field"));
        }
        let (field, remaining) = self.remaining.split_at(length);
        self.remaining = remaining;
        Ok(field)
    }
}
