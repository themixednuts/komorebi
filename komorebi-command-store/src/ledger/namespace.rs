use drizzle::core::expr::and;
use drizzle::core::expr::eq;
use drizzle::error::DrizzleError;
use drizzle::sqlite::connection::SQLiteTransactionType;
use komorebi_protocol::InvocationNamespaceId;
use komorebi_protocol::PrincipalId;

use super::DurableInvocationLedger;
use super::LedgerError;
use super::is_missing;
use crate::model::LeaseDecision;
use crate::model::LeaseRequest;
use crate::model::MAX_LIVE_RECORDS_PER_NAMESPACE;
use crate::model::NamespaceRegistration;
use crate::model::NewLeaseDecision;
use crate::schema::InsertInvocationLeases;
use crate::schema::SelectInvocationLeases;
use crate::schema::UpdateInvocationLeases;
use crate::storage::StoredNamespaceId;
use crate::storage::StoredPrincipalId;
use crate::storage::StoredSequence;

impl DurableInvocationLedger {
    /// Atomically registers a caller-generated namespace and leases its first
    /// contiguous sequence range.
    ///
    /// # Errors
    ///
    /// Returns [`LedgerError`] if the typed durable transaction fails.
    pub fn lease_new(&mut self, request: LeaseRequest) -> Result<NewLeaseDecision, LedgerError> {
        let table = self.schema.leases;
        let namespace = StoredNamespaceId(request.namespace);
        let principal = StoredPrincipalId(request.principal);

        self.db
            .transaction(SQLiteTransactionType::Immediate, |transaction| {
                let existing: Result<SelectInvocationLeases, DrizzleError> = transaction
                    .select(())
                    .from(table)
                    .r#where(eq(table.namespace_id, namespace))
                    .get();
                match existing {
                    Ok(_) => return Ok(NewLeaseDecision::NamespaceCollision),
                    Err(error) if is_missing(&error) => {}
                    Err(error) => return Err(error),
                }

                let first = komorebi_protocol::InvocationSequence::try_from(1)
                    .map_err(|error| DrizzleError::ConversionError(error.to_string().into()))?;
                let next = first
                    .advance(request.count)
                    .map_err(|error| DrizzleError::ConversionError(error.to_string().into()))?;
                transaction
                    .insert(table)
                    .values([InsertInvocationLeases::new(
                        namespace,
                        principal,
                        StoredSequence(next),
                        StoredSequence(first),
                        0,
                    )])
                    .execute()?;
                Ok(NewLeaseDecision::Issued(
                    komorebi_protocol::InvocationLease::new(
                        request.namespace,
                        first,
                        request.count,
                        first,
                    ),
                ))
            })
            .map_err(Into::into)
    }

    /// Registers a manager-issued namespace for exactly one principal.
    ///
    /// # Errors
    ///
    /// Returns [`LedgerError`] if the typed durable transaction fails.
    pub fn register_namespace(
        &mut self,
        namespace: InvocationNamespaceId,
        principal: PrincipalId,
    ) -> Result<NamespaceRegistration, LedgerError> {
        let table = self.schema.leases;
        let namespace = StoredNamespaceId(namespace);
        let principal = StoredPrincipalId(principal);

        self.db
            .transaction(SQLiteTransactionType::Immediate, |transaction| {
                let existing: Result<SelectInvocationLeases, DrizzleError> = transaction
                    .select(())
                    .from(table)
                    .r#where(eq(table.namespace_id, namespace))
                    .get();

                match existing {
                    Ok(existing) if existing.principal == principal => {
                        Ok(NamespaceRegistration::Existing)
                    }
                    Ok(_) => Ok(NamespaceRegistration::PrincipalConflict),
                    Err(error) if is_missing(&error) => {
                        let first = StoredSequence(
                            komorebi_protocol::InvocationSequence::try_from(1).map_err(
                                |error| DrizzleError::ConversionError(error.to_string().into()),
                            )?,
                        );
                        transaction
                            .insert(table)
                            .values([InsertInvocationLeases::new(
                                namespace, principal, first, first, 0,
                            )])
                            .execute()?;
                        Ok(NamespaceRegistration::Registered)
                    }
                    Err(error) => Err(error),
                }
            })
            .map_err(Into::into)
    }

    /// Durably advances a namespace's leased sequence range.
    ///
    /// # Errors
    ///
    /// Returns [`LedgerError`] if the typed durable transaction fails.
    pub fn lease(&mut self, request: LeaseRequest) -> Result<LeaseDecision, LedgerError> {
        let table = self.schema.leases;
        let namespace = StoredNamespaceId(request.namespace);
        let principal = StoredPrincipalId(request.principal);

        self.db
            .transaction(SQLiteTransactionType::Immediate, |transaction| {
                let existing: SelectInvocationLeases = match transaction
                    .select(())
                    .from(table)
                    .r#where(eq(table.namespace_id, namespace))
                    .get()
                {
                    Ok(existing) => existing,
                    Err(error) if is_missing(&error) => {
                        return Ok(LeaseDecision::UnknownNamespace);
                    }
                    Err(error) => return Err(error),
                };

                if existing.principal != principal {
                    return Ok(LeaseDecision::PrincipalConflict);
                }
                if existing.record_count >= MAX_LIVE_RECORDS_PER_NAMESPACE {
                    return Ok(LeaseDecision::CapacityFull);
                }

                let first = existing.next_sequence.0;
                let Ok(next) = first.advance(request.count) else {
                    return Ok(LeaseDecision::SequenceExhausted);
                };
                let updated = transaction
                    .update(table)
                    .set(UpdateInvocationLeases::default().with_next_sequence(StoredSequence(next)))
                    .r#where(and(
                        eq(table.namespace_id, namespace),
                        eq(table.principal, principal),
                    ))
                    .execute()?;
                if updated != 1 {
                    return Err(DrizzleError::Other(
                        "namespace lease update lost ownership".into(),
                    ));
                }

                Ok(LeaseDecision::Issued(
                    komorebi_protocol::InvocationLease::new(
                        request.namespace,
                        first,
                        request.count,
                        existing.minimum_accepted.0,
                    ),
                ))
            })
            .map_err(Into::into)
    }
}
