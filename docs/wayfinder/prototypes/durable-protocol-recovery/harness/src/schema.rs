use std::borrow::Cow;

use drizzle::error::DrizzleError;
use drizzle::sqlite::prelude::*;
use drizzle::sqlite::traits::DrizzleSQLiteColumn;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct InvocationParameters {
    pub schema: u16,
    pub action: String,
    pub arguments: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct InvocationDocument {
    parameters: InvocationParameters,
    encoded: Vec<u8>,
}

impl InvocationDocument {
    pub fn new(parameters: InvocationParameters) -> Result<Self, serde_json::Error> {
        let encoded = serde_json::to_vec(&parameters)?;
        Ok(Self {
            parameters,
            encoded,
        })
    }

    pub const fn parameters(&self) -> &InvocationParameters {
        &self.parameters
    }
}

impl DrizzleSQLiteColumn for InvocationDocument {
    type SQLType = drizzle::sqlite::types::Blob;

    const SQL_TYPE: &'static str = "BLOB";

    fn decode(value: SQLiteValueRef<'_>) -> Result<Self, DrizzleError> {
        let SQLiteValueRef::Blob(encoded) = value else {
            return Err(DrizzleError::ConversionError(
                "invocation document must be stored as BLOB".into(),
            ));
        };
        let parameters = serde_json::from_slice(encoded)?;
        Ok(Self {
            parameters,
            encoded: encoded.to_vec(),
        })
    }

    fn encode(&self) -> SQLiteValue<'_> {
        SQLiteValue::Blob(Cow::Borrowed(&self.encoded))
    }

    fn encode_owned(self) -> OwnedSQLiteValue {
        OwnedSQLiteValue::Blob(self.encoded.into_boxed_slice())
    }
}

#[SQLiteTable(name = "InvocationLedger")]
pub struct InvocationLedgerRow {
    #[column(primary)]
    pub identity: String,
    pub principal: String,
    pub invocation_id: i64,
    pub digest: Vec<u8>,
    pub phase: String,
    pub manager_revision: i64,
    pub effect_kind: String,
    pub outcome: Option<String>,
    #[column(blob)]
    pub parameters: InvocationDocument,
}

#[SQLiteTable(name = "PrincipalFloors")]
pub struct PrincipalFloorRow {
    #[column(primary)]
    pub principal: String,
    pub minimum_accepted: i64,
}

#[SQLiteTable(name = "CommittedEvents")]
pub struct CommittedEventRow {
    #[column(primary)]
    pub position: i64,
    pub manager_epoch: Vec<u8>,
    pub manager_revision: i64,
    pub invocation_identity: String,
    pub topic: String,
}

#[allow(clippy::expl_impl_clone_on_copy)]
#[derive(SQLiteSchema)]
pub struct ProtocolSchema {
    pub invocations: InvocationLedgerRow,
    pub principal_floors: PrincipalFloorRow,
    pub events: CommittedEventRow,
}
