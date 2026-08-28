use std::ffi::{OsStr, OsString};
use std::io::Cursor;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::domain::{PublicationFence, RequestId, ResultLimit, SearchText};
use crate::fff::{
    ContentSearchLimits, ContentSearchMeasurement, NameSearchMeasurement, SnapshotBuildMeasurement,
};

const MAX_FRAME_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WidePath(Vec<u16>);

impl WidePath {
    #[must_use]
    pub fn from_path(path: &Path) -> Self {
        Self(path.as_os_str().encode_wide().collect())
    }

    #[must_use]
    pub fn into_path(self) -> PathBuf {
        PathBuf::from(OsString::from_wide(&self.0))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkerRequest {
    Build {
        root: WidePath,
    },
    SearchName {
        fence: PublicationFence,
        query: SearchText,
        limit: ResultLimit,
    },
    SearchContent {
        fence: PublicationFence,
        query: SearchText,
        limits: ContentSearchLimits,
    },
    Crash,
    Hang,
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerRequestEnvelope {
    pub request_id: RequestId,
    pub request: WorkerRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkerResponse {
    Built(SnapshotBuildMeasurement),
    Name {
        fence: PublicationFence,
        measurement: NameSearchMeasurement,
    },
    Content {
        fence: PublicationFence,
        measurement: ContentSearchMeasurement,
    },
    Rejected(WorkerFailure),
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkerWireResponse {
    Ready {
        process_id: u32,
    },
    Reply {
        request_id: RequestId,
        response: WorkerResponse,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkerFailure {
    SnapshotMissing,
    Dependency,
    Protocol,
}

pub async fn write_frame<W, T>(writer: &mut W, value: &T) -> Result<(), ProtocolError>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let mut body = Vec::new();
    ciborium::into_writer(value, &mut body).map_err(|_| ProtocolError::Encode)?;
    if body.len() > MAX_FRAME_BYTES {
        return Err(ProtocolError::FrameTooLarge);
    }
    let length = u32::try_from(body.len()).map_err(|_| ProtocolError::FrameTooLarge)?;
    writer.write_all(&length.to_le_bytes()).await?;
    writer.write_all(&body).await?;
    writer.flush().await?;
    Ok(())
}

pub async fn read_frame<R, T>(reader: &mut R) -> Result<T, ProtocolError>
where
    R: AsyncRead + Unpin,
    T: DeserializeOwned,
{
    let mut length = [0u8; size_of::<u32>()];
    reader.read_exact(&mut length).await?;
    let length =
        usize::try_from(u32::from_le_bytes(length)).map_err(|_| ProtocolError::FrameTooLarge)?;
    if length > MAX_FRAME_BYTES {
        return Err(ProtocolError::FrameTooLarge);
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).await?;
    ciborium::from_reader(Cursor::new(body)).map_err(|_| ProtocolError::Decode)
}

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("worker transport failed")]
    Io(#[from] std::io::Error),
    #[error("worker frame exceeds its fixed bound")]
    FrameTooLarge,
    #[error("worker frame encoding failed")]
    Encode,
    #[error("worker frame decoding failed")]
    Decode,
}

#[allow(dead_code)]
fn _native_string_marker(_: &OsStr) {}
