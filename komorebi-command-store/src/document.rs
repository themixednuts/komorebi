use std::borrow::Cow;
use std::num::NonZeroU16;

use drizzle::error::DrizzleError;
use drizzle::sqlite::prelude::OwnedSQLiteValue;
use drizzle::sqlite::prelude::SQLiteValue;
use drizzle::sqlite::prelude::SQLiteValueRef;
use drizzle::sqlite::traits::DrizzleSQLiteColumn;
use rusqlite::types::FromSql;
use rusqlite::types::FromSqlResult;
use rusqlite::types::ValueRef;
use thiserror::Error;

use crate::storage::decode_from_rusqlite;

const VERSION_BYTES: usize = size_of::<u16>();
const MAX_DOCUMENT_BYTES: usize = 1024 * 1024;

macro_rules! versioned_document {
    ($name:ident, $label:literal) => {
        #[derive(Clone, Debug, Eq, PartialEq)]
        pub struct $name {
            schema_version: NonZeroU16,
            payload: Box<[u8]>,
        }

        impl $name {
            pub const MAX_BYTES: usize = MAX_DOCUMENT_BYTES;

            /// Creates a bounded, versioned document.
            ///
            /// # Errors
            ///
            /// Returns [`DocumentError::TooLarge`] when the version and payload
            /// exceed the durable document ceiling.
            pub fn new(
                schema_version: NonZeroU16,
                payload: impl Into<Box<[u8]>>,
            ) -> Result<Self, DocumentError> {
                let payload = payload.into();
                let encoded_len = VERSION_BYTES.saturating_add(payload.len());
                if encoded_len > MAX_DOCUMENT_BYTES {
                    return Err(DocumentError::TooLarge {
                        kind: $label,
                        actual: encoded_len,
                    });
                }

                Ok(Self {
                    schema_version,
                    payload,
                })
            }

            #[must_use]
            pub const fn schema_version(&self) -> NonZeroU16 {
                self.schema_version
            }

            #[must_use]
            pub const fn payload(&self) -> &[u8] {
                &self.payload
            }

            fn encode_bytes(&self) -> Vec<u8> {
                let mut encoded = Vec::with_capacity(VERSION_BYTES + self.payload.len());
                encoded.extend_from_slice(&self.schema_version.get().to_be_bytes());
                encoded.extend_from_slice(&self.payload);
                encoded
            }

            fn decode_bytes(bytes: &[u8]) -> Result<Self, DrizzleError> {
                if !(VERSION_BYTES..=MAX_DOCUMENT_BYTES).contains(&bytes.len()) {
                    return Err(DrizzleError::ConversionError(
                        format!(
                            "{} document has {} bytes; expected {VERSION_BYTES}..={MAX_DOCUMENT_BYTES}",
                            $label,
                            bytes.len()
                        )
                        .into(),
                    ));
                }

                let version_bytes: [u8; VERSION_BYTES] = bytes[..VERSION_BYTES]
                    .try_into()
                    .map_err(|_| DrizzleError::ConversionError("invalid document version".into()))?;
                let schema_version = NonZeroU16::new(u16::from_be_bytes(version_bytes))
                    .ok_or_else(|| DrizzleError::ConversionError("document schema version is zero".into()))?;

                Ok(Self {
                    schema_version,
                    payload: bytes[VERSION_BYTES..].into(),
                })
            }
        }

        impl DrizzleSQLiteColumn for $name {
            type SQLType = drizzle::sqlite::types::Blob;

            fn decode(value: SQLiteValueRef<'_>) -> Result<Self, DrizzleError> {
                let SQLiteValueRef::Blob(bytes) = value else {
                    return Err(DrizzleError::ConversionError(
                        concat!($label, " document must use SQLite BLOB storage").into(),
                    ));
                };
                Self::decode_bytes(bytes)
            }

            fn encode(&self) -> SQLiteValue<'_> {
                SQLiteValue::Blob(Cow::Owned(self.encode_bytes()))
            }

            fn encode_owned(self) -> OwnedSQLiteValue {
                let mut encoded = Vec::with_capacity(VERSION_BYTES + self.payload.len());
                encoded.extend_from_slice(&self.schema_version.get().to_be_bytes());
                encoded.extend_from_slice(&self.payload);
                OwnedSQLiteValue::Blob(encoded.into_boxed_slice())
            }
        }

        impl FromSql for $name {
            fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
                decode_from_rusqlite(value)
            }
        }
    };
}

versioned_document!(ActionParameterDocument, "action parameter");
versioned_document!(OutcomeDocument, "outcome");
versioned_document!(CommittedEventDocument, "committed event");

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum DocumentError {
    #[error("{kind} document has {actual} bytes; maximum is {MAX_DOCUMENT_BYTES}")]
    TooLarge { kind: &'static str, actual: usize },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_blob_round_trips_and_rejects_storage_mismatch() -> Result<(), DrizzleError> {
        let document = ActionParameterDocument::new(NonZeroU16::MIN, [1, 2, 3])
            .map_err(|error| DrizzleError::ConversionError(error.to_string().into()))?;
        let encoded = document.clone().encode_owned();
        let OwnedSQLiteValue::Blob(encoded) = encoded else {
            return Err(DrizzleError::ConversionError(
                "document did not encode as BLOB".into(),
            ));
        };

        assert_eq!(
            ActionParameterDocument::decode(SQLiteValueRef::Blob(&encoded))?,
            document
        );
        assert!(ActionParameterDocument::decode(SQLiteValueRef::Text("1")).is_err());
        assert!(ActionParameterDocument::decode(SQLiteValueRef::Blob(&[0, 0])).is_err());
        Ok(())
    }
}
