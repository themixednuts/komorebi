use std::io::{self, Read, Write};
use std::num::{NonZeroU64, NonZeroUsize};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameLimit(NonZeroUsize);

impl FrameLimit {
    /// Creates a nonzero frame limit that fits the protocol's 32-bit length prefix.
    ///
    /// # Errors
    ///
    /// Returns an error when `bytes` is zero or exceeds `u32::MAX`.
    pub fn new(bytes: usize) -> io::Result<Self> {
        let limit = NonZeroUsize::new(bytes)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "zero frame limit"))?;
        u32::try_from(bytes)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "frame limit exceeds u32"))?;
        Ok(Self(limit))
    }

    #[must_use]
    pub const fn bytes(self) -> usize {
        self.0.get()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FrameCodec {
    limit: FrameLimit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ExtensionGeneration(NonZeroU64);

impl ExtensionGeneration {
    /// Creates a nonzero extension generation.
    ///
    /// # Errors
    ///
    /// Returns an error when `value` is zero.
    pub fn new(value: u64) -> io::Result<Self> {
        NonZeroU64::new(value)
            .map(Self)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "zero generation"))
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }

    #[must_use]
    pub const fn previous(self) -> Option<Self> {
        match NonZeroU64::new(self.get() - 1) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Returns the following extension generation.
    ///
    /// # Errors
    ///
    /// Returns an error when the generation is already `u64::MAX`.
    pub fn next(self) -> io::Result<Self> {
        self.get()
            .checked_add(1)
            .and_then(NonZeroU64::new)
            .map(Self)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "generation overflow"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParentExitMode {
    Graceful,
    Abort,
}

impl ParentExitMode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Graceful => "graceful",
            Self::Abort => "abort",
        }
    }
}

impl FromStr for ParentExitMode {
    type Err = io::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "graceful" => Ok(Self::Graceful),
            "abort" => Ok(Self::Abort),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unknown parent exit mode",
            )),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ParentControlFrame {
    Ready {
        nonce: Uuid,
        mode: ParentExitMode,
        child_pid: u32,
        profile_names: [String; 2],
    },
    Acknowledge {
        nonce: Uuid,
    },
}

impl FrameCodec {
    #[must_use]
    pub const fn new(limit: FrameLimit) -> Self {
        Self { limit }
    }

    /// Writes one length-prefixed JSON frame within this codec's limit.
    ///
    /// # Errors
    ///
    /// Returns an error when serialization fails, the payload exceeds the configured limit, or
    /// the writer fails.
    pub fn write(&self, writer: &mut impl Write, frame: &impl Serialize) -> io::Result<()> {
        let payload = serde_json::to_vec(frame).map_err(io::Error::other)?;
        if payload.len() > self.limit.bytes() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "frame too large",
            ));
        }
        let length = u32::try_from(payload.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "frame length overflow"))?;
        writer.write_all(&length.to_le_bytes())?;
        writer.write_all(&payload)?;
        writer.flush()
    }

    /// Reads one length-prefixed JSON frame within this codec's limit.
    ///
    /// # Errors
    ///
    /// Returns an error when the declared length exceeds the configured limit, the reader fails,
    /// or the payload is not valid JSON for `T`.
    pub fn read<T: for<'de> Deserialize<'de>>(&self, reader: &mut impl Read) -> io::Result<T> {
        let mut length = [0_u8; 4];
        reader.read_exact(&mut length)?;
        let length = u32::from_le_bytes(length) as usize;
        if length > self.limit.bytes() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "frame too large",
            ));
        }
        let mut payload = vec![0_u8; length];
        reader.read_exact(&mut payload)?;
        serde_json::from_slice(&payload).map_err(io::Error::other)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChildFrame {
    Hello {
        nonce: Uuid,
        runtime: RuntimeKind,
        facts: ChildFacts,
    },
    ProbeReport {
        generation: ExtensionGeneration,
        probes: Vec<ProbeOutcome>,
    },
    FaultArmed {
        generation: ExtensionGeneration,
        scenario: FaultScenario,
    },
    Echo {
        generation: ExtensionGeneration,
        sequence: u64,
        sent_ticks: u64,
    },
    StoragePut {
        generation: ExtensionGeneration,
        request: u64,
        key: String,
        expected_revision: u64,
        value: Vec<u8>,
    },
    StorageGet {
        generation: ExtensionGeneration,
        request: u64,
        key: String,
    },
    HttpGet {
        generation: ExtensionGeneration,
        request: u64,
        url: String,
    },
    Goodbye {
        generation: ExtensionGeneration,
        echo_rtt_us: Vec<f64>,
    },
}

impl ChildFrame {
    #[must_use]
    pub const fn generation(&self) -> Option<ExtensionGeneration> {
        match self {
            Self::Hello { .. } => None,
            Self::ProbeReport { generation, .. }
            | Self::FaultArmed { generation, .. }
            | Self::Echo { generation, .. }
            | Self::StoragePut { generation, .. }
            | Self::StorageGet { generation, .. }
            | Self::HttpGet { generation, .. }
            | Self::Goodbye { generation, .. } => Some(*generation),
        }
    }

    #[must_use]
    pub const fn request_id(&self) -> Option<u64> {
        match self {
            Self::StoragePut { request, .. }
            | Self::StorageGet { request, .. }
            | Self::HttpGet { request, .. } => Some(*request),
            Self::Hello { .. }
            | Self::ProbeReport { .. }
            | Self::FaultArmed { .. }
            | Self::Echo { .. }
            | Self::Goodbye { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HostFrame {
    Welcome {
        generation: ExtensionGeneration,
    },
    Echoed {
        sequence: u64,
        sent_ticks: u64,
    },
    StorageStored {
        request: u64,
        revision: u64,
    },
    StorageValue {
        request: u64,
        revision: u64,
        value: Vec<u8>,
    },
    HttpResult {
        request: u64,
        status: u16,
        bytes: usize,
    },
    Rejected {
        request: Option<u64>,
        code: String,
    },
    Cancel {
        generation: ExtensionGeneration,
    },
    RunFault {
        generation: ExtensionGeneration,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeKind {
    Rust,
    LuaJit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FaultScenario {
    CpuLoop,
    AllocationPressure,
    Deadlock,
    IndefiniteWait,
    PipeStall,
    Disconnect,
    LuaJitNativeCrash,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildFacts {
    pub pid: u32,
    pub app_container: bool,
    pub less_privileged_app_container: bool,
    pub package_sid: String,
    pub dll_search_hardened: bool,
    pub environment_keys: Vec<WindowsStringEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowsStringEvidence {
    pub utf8: Option<String>,
    pub utf16_code_units_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeOutcome {
    pub name: String,
    pub expected: ExpectedOutcome,
    pub observed: ObservedOutcome,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpectedOutcome {
    Allowed,
    Denied,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum ObservedOutcome {
    Allowed,
    Denied { os_error: Option<i32> },
    Unavailable { reason: String },
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, ErrorKind};

    use serde::{Deserialize, Serialize};

    use super::{ExtensionGeneration, FrameCodec, FrameLimit};

    const TEST_LIMIT: usize = 64 * 1024;

    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct Message {
        value: String,
    }

    #[test]
    fn frame_round_trips() {
        let expected = Message {
            value: "bounded".to_owned(),
        };
        let mut bytes = Vec::new();
        let codec = FrameCodec::new(FrameLimit::new(TEST_LIMIT).expect("valid test limit"));

        codec
            .write(&mut bytes, &expected)
            .expect("serialize bounded test frame");
        let actual: Message = codec
            .read(&mut Cursor::new(bytes))
            .expect("deserialize bounded test frame");

        assert_eq!(actual, expected);
    }

    #[test]
    fn oversized_serialized_frame_is_rejected() {
        let mut bytes = Vec::new();
        let codec = FrameCodec::new(FrameLimit::new(TEST_LIMIT).expect("valid test limit"));
        let frame = Message {
            value: "x".repeat(TEST_LIMIT),
        };

        let error = codec
            .write(&mut bytes, &frame)
            .expect_err("reject oversized frame");

        assert_eq!(error.kind(), ErrorKind::InvalidData);
        assert!(bytes.is_empty());
    }

    #[test]
    fn oversized_declared_frame_is_rejected_before_allocation() {
        let codec = FrameCodec::new(FrameLimit::new(TEST_LIMIT).expect("valid test limit"));
        let declared = u32::try_from(TEST_LIMIT + 1)
            .expect("maximum test frame length fits u32")
            .to_le_bytes();

        let error = codec
            .read::<Message>(&mut Cursor::new(declared))
            .expect_err("reject oversized declared frame");

        assert_eq!(error.kind(), ErrorKind::InvalidData);
    }

    #[test]
    fn malformed_json_is_rejected() {
        let payload = b"not-json";
        let mut bytes = u32::try_from(payload.len())
            .expect("test payload length fits u32")
            .to_le_bytes()
            .to_vec();
        bytes.extend_from_slice(payload);
        let codec = FrameCodec::new(FrameLimit::new(TEST_LIMIT).expect("valid test limit"));

        let error = codec
            .read::<Message>(&mut Cursor::new(bytes))
            .expect_err("reject malformed JSON");

        assert_eq!(error.kind(), ErrorKind::Other);
    }

    #[test]
    fn generation_is_nonzero_and_has_typed_predecessor() {
        assert!(ExtensionGeneration::new(0).is_err());
        let current = ExtensionGeneration::new(2).expect("valid current generation");
        assert_eq!(current.previous().map(ExtensionGeneration::get), Some(1));
        assert_eq!(current.next().expect("generation should advance").get(), 3);
        assert!(
            ExtensionGeneration::new(u64::MAX)
                .expect("maximum generation is nonzero")
                .next()
                .is_err()
        );
    }
}
