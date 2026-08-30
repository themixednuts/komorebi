use thiserror::Error;

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum CatalogContractError {
    #[error("{field} has {actual} entries; maximum is {maximum}")]
    TooMany {
        field: &'static str,
        actual: usize,
        maximum: usize,
    },
    #[error("an action definition must permit at least one use")]
    NoPermittedUses,
    #[error("action definition title and description must not be blank")]
    EmptyDefinitionText,
    #[error("an action definition repeats a keyword")]
    DuplicateKeyword,
    #[error("an action definition repeats a permitted use")]
    DuplicatePermittedUse,
    #[error("an action definition repeats a parameter ID")]
    DuplicateParameter,
    #[error("dynamic parameter choices must not be empty")]
    EmptyDynamicChoices,
    #[error("a dynamic parameter group repeats a choice")]
    DuplicateDynamicChoice,
    #[error("an action offer repeats a dynamic parameter group")]
    DuplicateDynamicChoiceGroup,
    #[error("an action offer repeats a binding hint")]
    DuplicateBindingHint,
    #[error("catalog values belong to different manager epochs")]
    EpochMismatch,
    #[error("catalog definition and offer counts differ")]
    DefinitionOfferCountMismatch,
    #[error("catalog repeats an action key")]
    DuplicateAction,
    #[error("an offer does not belong to its enclosing snapshot")]
    OfferOutsideSnapshot,
}

pub(super) fn bounded(
    field: &'static str,
    actual: usize,
    maximum: usize,
) -> Result<(), CatalogContractError> {
    if actual > maximum {
        Err(CatalogContractError::TooMany {
            field,
            actual,
            maximum,
        })
    } else {
        Ok(())
    }
}

pub(super) fn has_duplicate<T: PartialEq>(values: &[T]) -> bool {
    values
        .iter()
        .enumerate()
        .any(|(index, value)| values[index + 1..].contains(value))
}

pub(super) fn has_duplicate_by<T, K: PartialEq>(values: &[T], key: impl Fn(&T) -> &K) -> bool {
    values.iter().enumerate().any(|(index, value)| {
        values[index + 1..]
            .iter()
            .any(|candidate| key(candidate) == key(value))
    })
}
