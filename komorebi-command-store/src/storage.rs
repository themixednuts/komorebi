use drizzle::error::DrizzleError;
use drizzle::sqlite::prelude::OwnedSQLiteValue;
use drizzle::sqlite::prelude::SQLiteValue;
use drizzle::sqlite::prelude::SQLiteValueRef;
use drizzle::sqlite::traits::DrizzleSQLiteColumn;
use komorebi_protocol::InvocationDigest;
use komorebi_protocol::InvocationIdentityError;
use komorebi_protocol::InvocationNamespaceId;
use komorebi_protocol::InvocationSequence;
use komorebi_protocol::ManagerEpoch;
use komorebi_protocol::PrincipalId;
use komorebi_protocol::Revision;
use komorebi_protocol::StateStamp;
use rusqlite::types::FromSql;
use rusqlite::types::FromSqlError;
use rusqlite::types::FromSqlResult;
use rusqlite::types::ValueRef;
use std::borrow::Cow;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct StoredNamespaceId(pub InvocationNamespaceId);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct StoredPrincipalId(pub PrincipalId);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct StoredDigest(pub InvocationDigest);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) struct StoredSequence(pub InvocationSequence);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct StoredStateStamp(pub StateStamp);

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
impl DrizzleSQLiteColumn for StoredStateStamp {
    type SQLType = drizzle::sqlite::types::Blob;

    fn decode(value: SQLiteValueRef<'_>) -> Result<Self, DrizzleError> {
        let bytes = decode_blob::<24>(value, "committed state stamp")?;
        let epoch = ManagerEpoch::new(
            bytes[..16]
                .try_into()
                .map_err(|_| DrizzleError::ConversionError("invalid manager epoch".into()))?,
        )
        .map_err(|error| DrizzleError::ConversionError(error.to_string().into()))?;
        let revision = Revision::try_from(u64::from_be_bytes(
            bytes[16..]
                .try_into()
                .map_err(|_| DrizzleError::ConversionError("invalid revision".into()))?,
        ))
        .map_err(|error| DrizzleError::ConversionError(error.to_string().into()))?;
        Ok(Self(StateStamp::new(epoch, revision)))
    }

    fn encode(&self) -> SQLiteValue<'_> {
        SQLiteValue::Blob(Cow::Owned(encode_state_stamp(self.0).to_vec()))
    }

    fn encode_owned(self) -> OwnedSQLiteValue {
        OwnedSQLiteValue::Blob(Box::new(encode_state_stamp(self.0)))
    }
}

impl FromSql for StoredStateStamp {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        decode_from_rusqlite(value)
    }
}

fn encode_state_stamp(stamp: StateStamp) -> [u8; 24] {
    let mut bytes = [0; 24];
    bytes[..16].copy_from_slice(&stamp.epoch().into_bytes());
    bytes[16..].copy_from_slice(&stamp.revision().get().to_be_bytes());
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_stamp_blob_round_trips_and_rejects_epochless_revisions() -> Result<(), DrizzleError> {
        let stamp = StoredStateStamp(StateStamp::new(
            ManagerEpoch::new([7; 16])
                .map_err(|error| DrizzleError::ConversionError(error.to_string().into()))?,
            Revision::try_from(13)
                .map_err(|error| DrizzleError::ConversionError(error.to_string().into()))?,
        ));
        let OwnedSQLiteValue::Blob(encoded) = stamp.encode_owned() else {
            return Err(DrizzleError::ConversionError(
                "state stamp did not encode as BLOB".into(),
            ));
        };

        assert_eq!(
            StoredStateStamp::decode(SQLiteValueRef::Blob(&encoded))?,
            stamp
        );
        assert!(StoredStateStamp::decode(SQLiteValueRef::Blob(&[0; 8])).is_err());
        assert!(StoredStateStamp::decode(SQLiteValueRef::Blob(&[0; 24])).is_err());
        assert!(StoredStateStamp::decode(SQLiteValueRef::Integer(13)).is_err());
        Ok(())
    }
}
