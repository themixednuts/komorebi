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
use crate::schema::InsertInvocationNamespaces;
use crate::schema::SelectInvocationNamespaces;
use crate::schema::UpdateInvocationNamespaces;
use crate::storage::StoredNamespaceId;
use crate::storage::StoredPrincipalId;
use crate::storage::StoredSequence;

impl DurableInvocationLedger {
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
        let table = self.schema.namespaces;
        let namespace = StoredNamespaceId(namespace);
        let principal = StoredPrincipalId(principal);

        self.db
            .transaction(SQLiteTransactionType::Immediate, |transaction| {
                let existing: Result<SelectInvocationNamespaces, DrizzleError> = transaction
                    .select(())
                    .from(table)
                    .r#where(eq(table.namespace, namespace))
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
                            .values([InsertInvocationNamespaces::new(
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
        let table = self.schema.namespaces;
        let namespace = StoredNamespaceId(request.namespace);
        let principal = StoredPrincipalId(request.principal);

        self.db
            .transaction(SQLiteTransactionType::Immediate, |transaction| {
                let existing: SelectInvocationNamespaces = match transaction
                    .select(())
                    .from(table)
                    .r#where(eq(table.namespace, namespace))
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
                    .set(
                        UpdateInvocationNamespaces::default()
                            .with_next_sequence(StoredSequence(next)),
                    )
                    .r#where(and(
                        eq(table.namespace, namespace),
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
