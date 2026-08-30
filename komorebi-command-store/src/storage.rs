use std::borrow::Cow;
use std::num::NonZeroU64;

use drizzle::error::DrizzleError;
use drizzle::sqlite::prelude::OwnedSQLiteValue;
use drizzle::sqlite::prelude::SQLiteValue;
use drizzle::sqlite::prelude::SQLiteValueRef;
use drizzle::sqlite::traits::DrizzleSQLiteColumn;
use komorebi_protocol::InvocationDigest;
use komorebi_protocol::InvocationIdentityError;
use komorebi_protocol::InvocationNamespaceId;
use komorebi_protocol::InvocationSequence;
use komorebi_protocol::PrincipalId;
use rusqlite::types::FromSql;
use rusqlite::types::FromSqlError;
use rusqlite::types::FromSqlResult;
use rusqlite::types::ValueRef;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct CommittedRevision(NonZeroU64);

impl CommittedRevision {
    #[must_use]
    pub const fn new(value: NonZeroU64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct StoredNamespaceId(pub InvocationNamespaceId);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct StoredPrincipalId(pub PrincipalId);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct StoredDigest(pub InvocationDigest);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) struct StoredSequence(pub InvocationSequence);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) struct StoredRevision(pub CommittedRevision);

fn decode_blob<const N: usize>(
    value: SQLiteValueRef<'_>,
    label: &'static str,
) -> Result<[u8; N], DrizzleError> {
    let SQLiteValueRef::Blob(bytes) = value else {
        return Err(DrizzleError::ConversionError(
            format!("{label} must use SQLite BLOB storage").into(),
        ));
    };
    bytes.try_into().map_err(|_| {
        DrizzleError::ConversionError(
            format!("{label} has {} bytes; expected {N}", bytes.len()).into(),
        )
    })
}

fn identity_error(error: InvocationIdentityError) -> DrizzleError {
    DrizzleError::ConversionError(error.to_string().into())
}

pub(crate) fn decode_from_rusqlite<T>(value: ValueRef<'_>) -> FromSqlResult<T>
where
    T: DrizzleSQLiteColumn,
{
    let value = match value {
        ValueRef::Null => SQLiteValueRef::Null,
        ValueRef::Integer(value) => SQLiteValueRef::Integer(value),
        ValueRef::Real(value) => SQLiteValueRef::Real(value),
        ValueRef::Text(value) => SQLiteValueRef::Text(
            std::str::from_utf8(value).map_err(|error| FromSqlError::Other(Box::new(error)))?,
        ),
        ValueRef::Blob(value) => SQLiteValueRef::Blob(value),
    };
    T::decode(value).map_err(|error| FromSqlError::Other(Box::new(error)))
}

macro_rules! fixed_blob_column {
    ($stored:ident, $size:literal, $label:literal, $into_bytes:expr, $decode:expr) => {
        impl DrizzleSQLiteColumn for $stored {
            type SQLType = drizzle::sqlite::types::Blob;

            fn decode(value: SQLiteValueRef<'_>) -> Result<Self, DrizzleError> {
                let bytes = decode_blob::<$size>(value, $label)?;
                ($decode)(bytes).map(Self)
            }

            fn encode(&self) -> SQLiteValue<'_> {
                let bytes: [u8; $size] = ($into_bytes)(self.0);
                SQLiteValue::Blob(Cow::Owned(bytes.to_vec()))
            }

            fn encode_owned(self) -> OwnedSQLiteValue {
                let bytes: [u8; $size] = ($into_bytes)(self.0);
                OwnedSQLiteValue::Blob(Box::new(bytes))
            }
        }

        impl FromSql for $stored {
            fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
                decode_from_rusqlite(value)
            }
        }
    };
}

fixed_blob_column!(
    StoredNamespaceId,
    16,
    "invocation namespace ID",
    InvocationNamespaceId::into_bytes,
    |bytes| InvocationNamespaceId::new(bytes).map_err(identity_error)
);
fixed_blob_column!(
    StoredPrincipalId,
    32,
    "principal ID",
    PrincipalId::into_bytes,
    |bytes| PrincipalId::new(bytes).map_err(identity_error)
);
fixed_blob_column!(
    StoredDigest,
    32,
    "invocation digest",
    InvocationDigest::into_bytes,
    |bytes| InvocationDigest::new(bytes).map_err(identity_error)
);
fixed_blob_column!(
    StoredSequence,
    8,
    "invocation sequence",
    |sequence: InvocationSequence| sequence.get().to_be_bytes(),
    |bytes| InvocationSequence::try_from(u64::from_be_bytes(bytes)).map_err(identity_error)
);
fixed_blob_column!(
    StoredRevision,
    8,
    "committed revision",
    |revision: CommittedRevision| revision.get().to_be_bytes(),
    |bytes| NonZeroU64::new(u64::from_be_bytes(bytes))
        .map(CommittedRevision)
        .ok_or_else(|| DrizzleError::ConversionError("committed revision is zero".into()))
);
