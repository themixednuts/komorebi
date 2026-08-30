use thiserror::Error;

use crate::frame::HEADER_BYTES;
use crate::frame::MAX_FRAME_PAYLOAD_BYTES;

#[derive(Debug, Error, Eq, PartialEq)]
pub enum FrameError {
    #[error("protocol preface is not KCMD framing version 1")]
    InvalidPreface,
    #[error("frame header must be exactly {HEADER_BYTES} bytes, received {0}")]
    HeaderLength(usize),
    #[error("frame header field is truncated")]
    HeaderField,
    #[error("frame payload is {0} bytes; maximum is {MAX_FRAME_PAYLOAD_BYTES}")]
    PayloadTooLarge(usize),
    #[error("frame declares {declared} payload bytes but carries {actual}")]
    PayloadLengthMismatch { declared: usize, actual: usize },
    #[error("frame contains unknown version 1 flags {0:#06x}")]
    UnknownFlags(u16),
    #[error("frame reserved field must be zero, received {0:#010x}")]
    ReservedField(u32),
    #[error("direction sequence numbers begin at one")]
    ZeroDirectionSequence,
    #[error("client-initiated stream ID must be odd and nonzero, received {0}")]
    InvalidClientStream(u32),
    #[error("server-initiated stream ID must be even and nonzero, received {0}")]
    InvalidServerStream(u32),
}
