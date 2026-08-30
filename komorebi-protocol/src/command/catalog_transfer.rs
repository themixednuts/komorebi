use std::num::NonZeroU32;

use sha2::Digest;
use sha2::Sha256;
use thiserror::Error;

use super::CatalogCodec;
use super::CatalogReply;
use super::CommandCodecError;
use crate::SessionLimits;

const DIGEST_BYTES: usize = 32;
const TOTAL_BYTES: usize = size_of::<u32>();
const OFFSET_BYTES: usize = size_of::<u32>();
const HEADER_BYTES: usize = DIGEST_BYTES + TOTAL_BYTES + OFFSET_BYTES;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CatalogPayloadDigest([u8; DIGEST_BYTES]);

impl CatalogPayloadDigest {
    fn of(payload: &[u8]) -> Self {
        Self(Sha256::digest(payload).into())
    }

    #[must_use]
    pub const fn into_bytes(self) -> [u8; DIGEST_BYTES] {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogChunk {
    digest: CatalogPayloadDigest,
    total: NonZeroU32,
    offset: u32,
    payload: Box<[u8]>,
}

impl CatalogChunk {
    #[must_use]
    pub const fn digest(&self) -> CatalogPayloadDigest {
        self.digest
    }

    #[must_use]
    pub const fn total(&self) -> NonZeroU32 {
        self.total
    }

    #[must_use]
    pub const fn offset(&self) -> u32 {
        self.offset
    }

    #[must_use]
    pub const fn payload(&self) -> &[u8] {
        &self.payload
    }

    #[must_use]
    pub fn encode(&self) -> Box<[u8]> {
        let mut bytes = Vec::with_capacity(HEADER_BYTES + self.payload.len());
        bytes.extend_from_slice(&self.digest.0);
        bytes.extend_from_slice(&self.total.get().to_be_bytes());
        bytes.extend_from_slice(&self.offset.to_be_bytes());
        bytes.extend_from_slice(&self.payload);
        bytes.into_boxed_slice()
    }

    /// Decodes one self-identifying catalog chunk under negotiated limits.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogTransferError`] for an oversized, truncated, empty, or
    /// out-of-bounds chunk.
    pub fn decode(bytes: &[u8], limits: SessionLimits) -> Result<Self, CatalogTransferError> {
        let maximum = usize::try_from(limits.chunk_payload().get())
            .map_err(|_| CatalogTransferError::AddressSpaceLimit)?;
        if bytes.len() > maximum {
            return Err(CatalogTransferError::ChunkTooLarge {
                actual: bytes.len(),
                maximum,
            });
        }
        if bytes.len() <= HEADER_BYTES {
            return Err(CatalogTransferError::TruncatedOrEmptyChunk);
        }

        let digest = CatalogPayloadDigest(
            bytes[..DIGEST_BYTES]
                .try_into()
                .map_err(|_| CatalogTransferError::TruncatedOrEmptyChunk)?,
        );
        let total = NonZeroU32::new(u32::from_be_bytes(
            bytes[DIGEST_BYTES..DIGEST_BYTES + TOTAL_BYTES]
                .try_into()
                .map_err(|_| CatalogTransferError::TruncatedOrEmptyChunk)?,
        ))
        .ok_or(CatalogTransferError::ZeroTotal)?;
        let offset_start = DIGEST_BYTES + TOTAL_BYTES;
        let offset = u32::from_be_bytes(
            bytes[offset_start..HEADER_BYTES]
                .try_into()
                .map_err(|_| CatalogTransferError::TruncatedOrEmptyChunk)?,
        );
        let payload = &bytes[HEADER_BYTES..];
        validate_extent(total, offset, payload.len(), limits)?;
        Ok(Self {
            digest,
            total,
            offset,
            payload: payload.into(),
        })
    }
}

/// Produces immutable, self-identifying frames for one catalog reply.
pub struct CatalogChunks {
    digest: CatalogPayloadDigest,
    payload: Box<[u8]>,
    data_limit: usize,
    offset: usize,
}

impl CatalogChunks {
    /// Canonically encodes a reply and prepares negotiated-size chunks.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogTransferError`] when the reply cannot be encoded, does
    /// not fit the negotiated reassembly limit, or the chunk limit cannot hold
    /// the fixed transfer header and at least one data byte.
    pub fn new(reply: &CatalogReply, limits: SessionLimits) -> Result<Self, CatalogTransferError> {
        let payload = CatalogCodec::encode_reply(reply)?.into_boxed_slice();
        let reassembly_limit = usize::try_from(limits.reassembly().get())
            .map_err(|_| CatalogTransferError::AddressSpaceLimit)?;
        if payload.len() > reassembly_limit {
            return Err(CatalogTransferError::LogicalPayloadTooLarge {
                actual: payload.len(),
                maximum: reassembly_limit,
            });
        }
        let chunk_limit = usize::try_from(limits.chunk_payload().get())
            .map_err(|_| CatalogTransferError::AddressSpaceLimit)?;
        let data_limit = chunk_limit
            .checked_sub(HEADER_BYTES)
            .filter(|limit| *limit > 0)
            .ok_or(CatalogTransferError::ChunkLimitTooSmall {
                actual: chunk_limit,
                minimum: HEADER_BYTES + 1,
            })?;
        Ok(Self {
            digest: CatalogPayloadDigest::of(&payload),
            payload,
            data_limit,
            offset: 0,
        })
    }

    /// Returns the next chunk, or `None` after the complete logical payload.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogTransferError::AddressSpaceLimit`] if the local target
    /// cannot represent a protocol-v1 payload extent.
    pub fn next_chunk(&mut self) -> Result<Option<CatalogChunk>, CatalogTransferError> {
        if self.offset == self.payload.len() {
            return Ok(None);
        }
        let end = self
            .offset
            .saturating_add(self.data_limit)
            .min(self.payload.len());
        let total = NonZeroU32::new(
            u32::try_from(self.payload.len())
                .map_err(|_| CatalogTransferError::AddressSpaceLimit)?,
        )
        .ok_or(CatalogTransferError::ZeroTotal)?;
        let offset =
            u32::try_from(self.offset).map_err(|_| CatalogTransferError::AddressSpaceLimit)?;
        let chunk = CatalogChunk {
            digest: self.digest,
            total,
            offset,
            payload: self.payload[self.offset..end].into(),
        };
        self.offset = end;
        Ok(Some(chunk))
    }
}

#[derive(Debug)]
pub struct CatalogReassembler {
    limits: SessionLimits,
    transfer: Option<PartialCatalog>,
}

impl CatalogReassembler {
    #[must_use]
    pub const fn new(limits: SessionLimits) -> Self {
        Self {
            limits,
            transfer: None,
        }
    }

    /// Accepts the next contiguous chunk and returns a verified typed reply
    /// exactly when the logical payload is complete.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogTransferError`] for malformed chunks, interleaved
    /// transfers, gaps, replayed offsets, digest mismatch, or invalid catalog
    /// bytes.
    pub fn push(&mut self, bytes: &[u8]) -> Result<Option<CatalogReply>, CatalogTransferError> {
        let chunk = CatalogChunk::decode(bytes, self.limits)?;
        let transfer = if let Some(transfer) = &mut self.transfer {
            transfer.accept(&chunk)?;
            transfer
        } else {
            if chunk.offset != 0 {
                return Err(CatalogTransferError::NonContiguous {
                    expected: 0,
                    actual: chunk.offset,
                });
            }
            self.transfer.insert(PartialCatalog::from_first(&chunk))
        };
        if transfer.payload.len()
            < usize::try_from(transfer.total.get())
                .map_err(|_| CatalogTransferError::AddressSpaceLimit)?
        {
            return Ok(None);
        }

        let completed = self
            .transfer
            .take()
            .ok_or(CatalogTransferError::MissingTransfer)?;
        if CatalogPayloadDigest::of(&completed.payload) != completed.digest {
            return Err(CatalogTransferError::DigestMismatch);
        }
        Ok(Some(CatalogCodec::decode_reply(&completed.payload)?))
    }

    #[must_use]
    pub const fn is_pending(&self) -> bool {
        self.transfer.is_some()
    }
}

#[derive(Debug)]
struct PartialCatalog {
    digest: CatalogPayloadDigest,
    total: NonZeroU32,
    payload: Vec<u8>,
}

impl PartialCatalog {
    fn from_first(chunk: &CatalogChunk) -> Self {
        Self {
            digest: chunk.digest,
            total: chunk.total,
            payload: chunk.payload.to_vec(),
        }
    }

    fn accept(&mut self, chunk: &CatalogChunk) -> Result<(), CatalogTransferError> {
        if self.digest != chunk.digest || self.total != chunk.total {
            return Err(CatalogTransferError::TransferIdentityChanged);
        }
        let expected = u32::try_from(self.payload.len())
            .map_err(|_| CatalogTransferError::AddressSpaceLimit)?;
        if chunk.offset != expected {
            return Err(CatalogTransferError::NonContiguous {
                expected,
                actual: chunk.offset,
            });
        }
        self.payload.extend_from_slice(&chunk.payload);
        Ok(())
    }
}

fn validate_extent(
    total: NonZeroU32,
    offset: u32,
    payload_len: usize,
    limits: SessionLimits,
) -> Result<(), CatalogTransferError> {
    if total.get() > limits.reassembly().get() {
        return Err(CatalogTransferError::LogicalPayloadTooLarge {
            actual: usize::try_from(total.get())
                .map_err(|_| CatalogTransferError::AddressSpaceLimit)?,
            maximum: usize::try_from(limits.reassembly().get())
                .map_err(|_| CatalogTransferError::AddressSpaceLimit)?,
        });
    }
    let payload_len =
        u32::try_from(payload_len).map_err(|_| CatalogTransferError::AddressSpaceLimit)?;
    let end = offset
        .checked_add(payload_len)
        .ok_or(CatalogTransferError::ChunkOutsideLogicalPayload)?;
    if offset >= total.get() || end > total.get() {
        Err(CatalogTransferError::ChunkOutsideLogicalPayload)
    } else {
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum CatalogTransferError {
    #[error("catalog chunk payload limit is {actual} bytes; minimum is {minimum}")]
    ChunkLimitTooSmall { actual: usize, minimum: usize },
    #[error("catalog chunk is {actual} bytes; negotiated maximum is {maximum}")]
    ChunkTooLarge { actual: usize, maximum: usize },
    #[error("catalog chunk has no complete header and nonempty payload")]
    TruncatedOrEmptyChunk,
    #[error("catalog transfer declares an empty logical payload")]
    ZeroTotal,
    #[error("catalog chunk lies outside its declared logical payload")]
    ChunkOutsideLogicalPayload,
    #[error("catalog logical payload is {actual} bytes; negotiated maximum is {maximum}")]
    LogicalPayloadTooLarge { actual: usize, maximum: usize },
    #[error("catalog transfer identity changed before completion")]
    TransferIdentityChanged,
    #[error("catalog chunks are not contiguous: expected offset {expected}, received {actual}")]
    NonContiguous { expected: u32, actual: u32 },
    #[error("catalog logical payload digest does not match its chunks")]
    DigestMismatch,
    #[error("catalog transfer completion lost its reassembly state")]
    MissingTransfer,
    #[error("catalog size is outside the local address space")]
    AddressSpaceLimit,
    #[error(transparent)]
    Codec(#[from] CommandCodecError),
}
