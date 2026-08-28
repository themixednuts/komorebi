use std::path::Path;

use drizzle::error::DrizzleError;
use drizzle::sqlite::prelude::asc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::domain::{Boundary, PromotionIdentity};
use crate::schema::{InsertPromotionJournalRow, SelectPromotionJournalRow};
use crate::store::{Store, StoreError};

const SCHEMA_VERSION: u8 = 1;
const GENESIS_DIGEST: &str = "genesis";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JournalRecord {
    pub schema_version: u8,
    pub sequence: u32,
    pub previous_digest: String,
    pub identity: PromotionIdentity,
    pub boundary: Boundary,
    pub digest: String,
}

pub struct Journal {
    store: Store,
    records: Vec<JournalRecord>,
}

impl Journal {
    pub fn open(path: &Path) -> Result<Self, JournalError> {
        let store = Store::open(path)?;
        let table = store.schema.journal;

        let rows: Vec<SelectPromotionJournalRow> = store
            .database
            .select(())
            .from(table)
            .order_by(asc(table.sequence))
            .all()
            .map_err(JournalError::Drizzle)?;
        let mut records = Vec::with_capacity(rows.len());
        for row in rows {
            let record = decode_row(row)?;
            validate_record(&records, &record)?;
            records.push(record);
        }
        Ok(Self { store, records })
    }

    pub fn records(&self) -> &[JournalRecord] {
        &self.records
    }

    pub fn last(&self) -> Option<&JournalRecord> {
        self.records.last()
    }

    pub fn append(
        &mut self,
        identity: &PromotionIdentity,
        boundary: Boundary,
    ) -> Result<&JournalRecord, JournalError> {
        let sequence = u32::try_from(self.records.len())
            .map_err(|_| JournalError::SequenceExhausted)?
            .checked_add(1)
            .ok_or(JournalError::SequenceExhausted)?;
        let previous_digest = self
            .records
            .last()
            .map_or_else(|| GENESIS_DIGEST.to_owned(), |record| record.digest.clone());
        let digest = record_digest(sequence, &previous_digest, identity, boundary)?;
        self.store
            .database
            .insert(self.store.schema.journal)
            .value(
                InsertPromotionJournalRow::new(
                    identity.transaction.clone(),
                    identity.prior.as_str().to_owned(),
                    identity.candidate.as_str().to_owned(),
                    identity.fault.as_str().to_owned(),
                    boundary.to_string(),
                    previous_digest.clone(),
                    digest.clone(),
                )
                .with_sequence(i64::from(sequence)),
            )
            .execute()
            .map_err(JournalError::Drizzle)?;
        self.records.push(JournalRecord {
            schema_version: SCHEMA_VERSION,
            sequence,
            previous_digest,
            identity: identity.clone(),
            boundary,
            digest,
        });
        self.records.last().ok_or(JournalError::SequenceExhausted)
    }
}

fn decode_row(row: SelectPromotionJournalRow) -> Result<JournalRecord, JournalError> {
    Ok(JournalRecord {
        schema_version: SCHEMA_VERSION,
        sequence: u32::try_from(row.sequence).map_err(|_| JournalError::SequenceRange)?,
        previous_digest: row.previous_digest,
        identity: PromotionIdentity {
            transaction: row.transaction,
            prior: row
                .prior_installation
                .parse()
                .map_err(JournalError::PriorId)?,
            candidate: row
                .candidate_installation
                .parse()
                .map_err(JournalError::CandidateId)?,
            fault: row
                .fault_profile
                .parse()
                .map_err(JournalError::FaultProfile)?,
        },
        boundary: row.boundary.parse().map_err(JournalError::Boundary)?,
        digest: row.digest,
    })
}

fn validate_record(prior: &[JournalRecord], record: &JournalRecord) -> Result<(), JournalError> {
    let expected_sequence = u32::try_from(prior.len())
        .map_err(|_| JournalError::SequenceExhausted)?
        .checked_add(1)
        .ok_or(JournalError::SequenceExhausted)?;
    if record.sequence != expected_sequence {
        return Err(JournalError::Sequence {
            expected: expected_sequence,
            actual: record.sequence,
        });
    }
    let expected_previous = prior
        .last()
        .map_or(GENESIS_DIGEST, |entry| entry.digest.as_str());
    if record.previous_digest != expected_previous {
        return Err(JournalError::BrokenChain(record.sequence));
    }
    let expected = record_digest(
        record.sequence,
        &record.previous_digest,
        &record.identity,
        record.boundary,
    )?;
    if record.digest != expected {
        return Err(JournalError::Digest(record.sequence));
    }
    Ok(())
}

fn record_digest(
    sequence: u32,
    previous_digest: &str,
    identity: &PromotionIdentity,
    boundary: Boundary,
) -> Result<String, JournalError> {
    let mut digest = Sha256::new();
    digest.update([SCHEMA_VERSION]);
    digest.update(sequence.to_le_bytes());
    update_framed(&mut digest, previous_digest.as_bytes())?;
    update_framed(&mut digest, identity.transaction.as_bytes())?;
    update_framed(&mut digest, identity.prior.as_str().as_bytes())?;
    update_framed(&mut digest, identity.candidate.as_str().as_bytes())?;
    update_framed(&mut digest, identity.fault.as_str().as_bytes())?;
    update_framed(&mut digest, boundary.as_str().as_bytes())?;
    Ok(hex::encode(digest.finalize()))
}

fn update_framed(digest: &mut Sha256, bytes: &[u8]) -> Result<(), JournalError> {
    let length = u64::try_from(bytes.len()).map_err(|_| JournalError::DigestMaterialTooLarge)?;
    digest.update(length.to_le_bytes());
    digest.update(bytes);
    Ok(())
}

#[derive(Debug, Error)]
pub enum JournalError {
    #[error("open durable store")]
    Store(#[from] StoreError),
    #[error("Drizzle durable-store operation")]
    Drizzle(#[source] DrizzleError),
    #[error("journal digest material exceeds its length encoding")]
    DigestMaterialTooLarge,
    #[error("journal sequence is outside the domain range")]
    SequenceRange,
    #[error("journal sequence exhausted")]
    SequenceExhausted,
    #[error("expected journal sequence {expected}, found {actual}")]
    Sequence { expected: u32, actual: u32 },
    #[error("journal hash chain is broken at sequence {0}")]
    BrokenChain(u32),
    #[error("journal digest mismatch at sequence {0}")]
    Digest(u32),
    #[error("invalid prior installation identifier in durable store")]
    PriorId(#[source] crate::domain::InvalidInstallationId),
    #[error("invalid candidate installation identifier in durable store")]
    CandidateId(#[source] crate::domain::InvalidInstallationId),
    #[error("invalid fault profile in durable store")]
    FaultProfile(#[source] crate::domain::InvalidFaultProfile),
    #[error("invalid boundary in durable store")]
    Boundary(#[source] crate::domain::InvalidBoundary),
}
