use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize};

pub const MAX_FRAME_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChildFrame {
    Hello {
        nonce: String,
        runtime: RuntimeKind,
        facts: ChildFacts,
    },
    ProbeReport {
        probes: Vec<ProbeOutcome>,
    },
    Echo {
        sequence: u64,
        sent_ticks: u64,
    },
    StoragePut {
        request: u64,
        key: String,
        expected_revision: u64,
        value: Vec<u8>,
    },
    StorageGet {
        request: u64,
        key: String,
    },
    HttpGet {
        request: u64,
        url: String,
    },
    Goodbye {
        echo_rtt_us: Vec<f64>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HostFrame {
    Welcome {
        generation: u64,
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
        generation: u64,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeKind {
    Rust,
    LuaJit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildFacts {
    pub pid: u32,
    pub app_container: bool,
    pub less_privileged_app_container: bool,
    pub package_sid: String,
    pub dll_search_hardened: bool,
    pub environment_keys: Vec<String>,
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

/// Writes one bounded, length-prefixed JSON frame.
///
/// # Errors
///
/// Returns an error when serialization fails, the payload exceeds the protocol limit, or the
/// writer fails.
pub fn write_frame(writer: &mut impl Write, frame: &impl Serialize) -> io::Result<()> {
    let payload = serde_json::to_vec(frame).map_err(io::Error::other)?;
    if payload.len() > MAX_FRAME_BYTES {
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

/// Reads one bounded, length-prefixed JSON frame.
///
/// # Errors
///
/// Returns an error when the declared length exceeds the protocol limit, the reader fails, or the
/// payload is not valid JSON for `T`.
pub fn read_frame<T: for<'de> Deserialize<'de>>(reader: &mut impl Read) -> io::Result<T> {
    let mut length = [0_u8; 4];
    reader.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if length > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame too large",
        ));
    }
    let mut payload = vec![0_u8; length];
    reader.read_exact(&mut payload)?;
    serde_json::from_slice(&payload).map_err(io::Error::other)
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, ErrorKind};

    use serde::{Deserialize, Serialize};

    use super::{MAX_FRAME_BYTES, read_frame, write_frame};

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

        write_frame(&mut bytes, &expected).expect("serialize bounded test frame");
        let actual: Message =
            read_frame(&mut Cursor::new(bytes)).expect("deserialize bounded test frame");

        assert_eq!(actual, expected);
    }

    #[test]
    fn oversized_serialized_frame_is_rejected() {
        let mut bytes = Vec::new();
        let frame = Message {
            value: "x".repeat(MAX_FRAME_BYTES),
        };

        let error = write_frame(&mut bytes, &frame).expect_err("reject oversized frame");

        assert_eq!(error.kind(), ErrorKind::InvalidData);
        assert!(bytes.is_empty());
    }

    #[test]
    fn oversized_declared_frame_is_rejected_before_allocation() {
        let declared = u32::try_from(MAX_FRAME_BYTES + 1)
            .expect("maximum test frame length fits u32")
            .to_le_bytes();

        let error = read_frame::<Message>(&mut Cursor::new(declared))
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

        let error =
            read_frame::<Message>(&mut Cursor::new(bytes)).expect_err("reject malformed JSON");

        assert_eq!(error.kind(), ErrorKind::Other);
    }
}
