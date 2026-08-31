use std::fmt;
use std::num::NonZeroU64;
use std::num::NonZeroUsize;

use thiserror::Error;

const MAX_PLUGIN_ID_BYTES: usize = 64;
const MAX_CHUNK_NAME_BYTES: usize = 256;

/// Stable extension identity used at every broker boundary.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct PluginId(Box<str>);

impl PluginId {
    /// Parses a lowercase ASCII identifier with no path or namespace syntax.
    pub fn parse(value: &str) -> Result<Self, PluginIdError> {
        let valid_length = !value.is_empty() && value.len() <= MAX_PLUGIN_ID_BYTES;
        let mut bytes = value.bytes();
        let valid_start = bytes.next().is_some_and(|byte| byte.is_ascii_lowercase());
        let valid_body =
            bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
        let valid_end = value
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric);
        if !(valid_length && valid_start && valid_body && valid_end) {
            return Err(PluginIdError);
        }
        Ok(Self(value.into()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PluginId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("plugin id must be 1-64 lowercase ASCII letters, digits, or interior hyphens")]
pub struct PluginIdError;

/// Broker authority that may be granted to one extension.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(u8)]
pub enum PluginCapability {
    Log,
    InvokeAction,
    ObserveWindows,
    ReadFiles,
    WebRequest,
}

impl PluginCapability {
    const fn mask(self) -> u32 {
        1 << (self as u8)
    }
}

/// Closed authority set; unavailable combinations cannot acquire ambient handles.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PluginCapabilitySet(u32);

impl PluginCapabilitySet {
    #[must_use]
    pub const fn empty() -> Self {
        Self(0)
    }

    #[must_use]
    pub fn only(capabilities: impl IntoIterator<Item = PluginCapability>) -> Self {
        capabilities
            .into_iter()
            .fold(Self::empty(), |set, capability| {
                Self(set.0 | capability.mask())
            })
    }

    #[must_use]
    pub const fn allows(self, capability: PluginCapability) -> bool {
        self.0 & capability.mask() != 0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PluginManifest {
    id: PluginId,
    capabilities: PluginCapabilitySet,
}

impl PluginManifest {
    #[must_use]
    pub const fn new(id: PluginId, capabilities: PluginCapabilitySet) -> Self {
        Self { id, capabilities }
    }

    #[must_use]
    pub const fn id(&self) -> &PluginId {
        &self.id
    }

    #[must_use]
    pub const fn capabilities(&self) -> PluginCapabilitySet {
        self.capabilities
    }

    pub(crate) fn into_parts(self) -> (PluginId, PluginCapabilitySet) {
        (self.id, self.capabilities)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MemoryBudget(NonZeroUsize);

impl MemoryBudget {
    #[must_use]
    pub const fn new(bytes: usize) -> Option<Self> {
        match NonZeroUsize::new(bytes) {
            Some(bytes) => Some(Self(bytes)),
            None => None,
        }
    }

    pub(crate) const fn bytes(self) -> usize {
        self.0.get()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InstructionBudget(NonZeroU64);

impl InstructionBudget {
    #[must_use]
    pub const fn new(instructions: u64) -> Option<Self> {
        match NonZeroU64::new(instructions) {
            Some(instructions) => Some(Self(instructions)),
            None => None,
        }
    }

    pub(crate) const fn instructions(self) -> u64 {
        self.0.get()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PluginLimits {
    memory: MemoryBudget,
    instructions: InstructionBudget,
}

impl PluginLimits {
    #[must_use]
    pub const fn new(memory: MemoryBudget, instructions: InstructionBudget) -> Self {
        Self {
            memory,
            instructions,
        }
    }

    pub(crate) const fn memory(self) -> MemoryBudget {
        self.memory
    }

    pub(crate) const fn instructions(self) -> InstructionBudget {
        self.instructions
    }
}

/// Script bytes plus a diagnostic-only chunk name; no filesystem authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PluginProgram {
    name: Box<str>,
    source: Box<[u8]>,
}

impl PluginProgram {
    pub fn new(name: &str, source: impl AsRef<[u8]>) -> Result<Self, PluginProgramError> {
        if name.is_empty() || name.len() > MAX_CHUNK_NAME_BYTES || name.contains('\0') {
            return Err(PluginProgramError::InvalidName);
        }
        let source = source.as_ref();
        std::str::from_utf8(source).map_err(|_| PluginProgramError::SourceNotUtf8)?;
        Ok(Self {
            name: name.into(),
            source: source.into(),
        })
    }

    pub(crate) fn into_parts(self) -> (Box<str>, Box<[u8]>) {
        (self.name, self.source)
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum PluginProgramError {
    #[error("chunk name must contain 1-256 bytes and no NUL")]
    InvalidName,
    #[error("plugin source must be UTF-8 text, not Lua bytecode")]
    SourceNotUtf8,
}
