use thiserror::Error;

const V1_FRAME_BYTES: u32 = 1024 * 1024;
const V1_CONTROL_BYTES: u32 = 16 * 1024;
const V1_CHUNK_BYTES: u32 = 64 * 1024;
const V1_REASSEMBLY_BYTES: u32 = 8 * 1024 * 1024;
const V1_NESTING_DEPTH: u8 = 32;
const V1_ASSEMBLY_DEADLINE_MS: u32 = 2_000;

macro_rules! byte_limit {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub struct $name(u32);

        impl $name {
            /// Creates a nonzero byte limit.
            ///
            /// # Errors
            ///
            /// Returns [`SessionLimitError::Zero`] when `value` is zero.
            pub fn new(value: u32) -> Result<Self, SessionLimitError> {
                if value == 0 {
                    Err(SessionLimitError::Zero)
                } else {
                    Ok(Self(value))
                }
            }

            #[must_use]
            pub const fn get(self) -> u32 {
                self.0
            }
        }
    };
}

byte_limit!(FramePayloadLimit);
byte_limit!(ControlPayloadLimit);
byte_limit!(ChunkPayloadLimit);
byte_limit!(ReassemblyLimit);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NestingLimit(u8);

impl NestingLimit {
    /// Creates a nonzero nesting limit.
    ///
    /// # Errors
    ///
    /// Returns [`SessionLimitError::Zero`] when `value` is zero.
    pub fn new(value: u8) -> Result<Self, SessionLimitError> {
        if value == 0 {
            Err(SessionLimitError::Zero)
        } else {
            Ok(Self(value))
        }
    }

    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AssemblyDeadlineMs(u32);

impl AssemblyDeadlineMs {
    /// Creates a nonzero logical-message assembly deadline in milliseconds.
    ///
    /// # Errors
    ///
    /// Returns [`SessionLimitError::Zero`] when `value` is zero.
    pub fn new(value: u32) -> Result<Self, SessionLimitError> {
        if value == 0 {
            Err(SessionLimitError::Zero)
        } else {
            Ok(Self(value))
        }
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionLimits {
    frame_payload: FramePayloadLimit,
    control_payload: ControlPayloadLimit,
    chunk_payload: ChunkPayloadLimit,
    reassembly: ReassemblyLimit,
    nesting: NestingLimit,
    assembly_deadline: AssemblyDeadlineMs,
}

impl SessionLimits {
    pub const V1: Self = Self {
        frame_payload: FramePayloadLimit(V1_FRAME_BYTES),
        control_payload: ControlPayloadLimit(V1_CONTROL_BYTES),
        chunk_payload: ChunkPayloadLimit(V1_CHUNK_BYTES),
        reassembly: ReassemblyLimit(V1_REASSEMBLY_BYTES),
        nesting: NestingLimit(V1_NESTING_DEPTH),
        assembly_deadline: AssemblyDeadlineMs(V1_ASSEMBLY_DEADLINE_MS),
    };

    /// Creates limits no larger than the protocol v1 ceilings.
    ///
    /// # Errors
    ///
    /// Returns a [`SessionLimitError`] for a ceiling violation or inconsistent
    /// payload hierarchy.
    pub fn new(
        frame_payload: FramePayloadLimit,
        control_payload: ControlPayloadLimit,
        chunk_payload: ChunkPayloadLimit,
        reassembly: ReassemblyLimit,
        nesting: NestingLimit,
        assembly_deadline: AssemblyDeadlineMs,
    ) -> Result<Self, SessionLimitError> {
        if frame_payload.get() > V1_FRAME_BYTES
            || control_payload.get() > V1_CONTROL_BYTES
            || chunk_payload.get() > V1_CHUNK_BYTES
            || reassembly.get() > V1_REASSEMBLY_BYTES
            || nesting.get() > V1_NESTING_DEPTH
            || assembly_deadline.get() > V1_ASSEMBLY_DEADLINE_MS
        {
            return Err(SessionLimitError::AboveV1Ceiling);
        }
        if control_payload.get() > frame_payload.get()
            || chunk_payload.get() > frame_payload.get()
            || frame_payload.get() > reassembly.get()
        {
            return Err(SessionLimitError::InconsistentPayloadHierarchy);
        }
        Ok(Self {
            frame_payload,
            control_payload,
            chunk_payload,
            reassembly,
            nesting,
            assembly_deadline,
        })
    }

    #[must_use]
    pub const fn frame_payload(self) -> FramePayloadLimit {
        self.frame_payload
    }

    #[must_use]
    pub const fn control_payload(self) -> ControlPayloadLimit {
        self.control_payload
    }

    #[must_use]
    pub const fn chunk_payload(self) -> ChunkPayloadLimit {
        self.chunk_payload
    }

    #[must_use]
    pub const fn reassembly(self) -> ReassemblyLimit {
        self.reassembly
    }

    #[must_use]
    pub const fn nesting(self) -> NestingLimit {
        self.nesting
    }

    #[must_use]
    pub const fn assembly_deadline(self) -> AssemblyDeadlineMs {
        self.assembly_deadline
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum SessionLimitError {
    #[error("session limits must be nonzero")]
    Zero,
    #[error("session limits exceed a protocol v1 ceiling")]
    AboveV1Ceiling,
    #[error("control and chunk payloads must fit a frame, which must fit reassembly")]
    InconsistentPayloadHierarchy,
}
