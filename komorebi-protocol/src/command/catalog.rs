use std::collections::BTreeSet;

use super::ActionKey;
use super::ArgumentScalar;
use super::BoundedText;
use super::CatalogStamp;
use super::OfferRef;
use super::ParameterId;
use super::StateStamp;
use super::catalog_error::CatalogContractError;
use super::catalog_error::bounded;
use super::catalog_error::has_duplicate;
use super::catalog_error::has_duplicate_by;

pub(crate) const MAX_CATALOG_ACTIONS: usize = 1_024;
pub(crate) const MAX_DEFINITION_KEYWORDS: usize = 32;
pub(crate) const MAX_DEFINITION_PARAMETERS: usize = 32;
pub(crate) const MAX_DYNAMIC_CHOICE_GROUPS: usize = 32;
pub(crate) const MAX_DYNAMIC_CHOICES: usize = 256;
pub(crate) const MAX_BINDING_HINTS: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActionCategory {
    Window = 1,
    Workspace = 2,
    Configuration = 3,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum PermittedUse {
    Interactive = 1,
    Automation = 2,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfirmationPolicy {
    None = 1,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UndoPolicy {
    None = 1,
    PriorManagerIntent = 2,
    ExactCapturedState = 3,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParameterDomain {
    Direction = 1,
    Axis = 2,
    Pixels = 3,
    WorkspaceSelector = 4,
    WindowSelector = 5,
    Layout = 6,
    Cycle = 7,
    Index = 8,
    Sizing = 9,
    Adjustment = 10,
    Flag = 11,
    Size = 12,
    Count = 13,
    Columns = 14,
    Name = 15,
    Path = 16,
    Behaviour = 17,
    Implementation = 18,
    Executable = 19,
    Identifier = 20,
    Ratios = 21,
    AtCount = 22,
    ResizeStep = 23,
    Alpha = 24,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ArgumentCardinality {
    RequiredScalar = 1,
    RequiredList = 2,
    OptionalScalar = 3,
    OptionalList = 4,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionParameter {
    id: ParameterId,
    domain: ParameterDomain,
    cardinality: ArgumentCardinality,
}

impl ActionParameter {
    #[must_use]
    pub const fn new(
        id: ParameterId,
        domain: ParameterDomain,
        cardinality: ArgumentCardinality,
    ) -> Self {
        Self {
            id,
            domain,
            cardinality,
        }
    }

    #[must_use]
    pub const fn id(&self) -> &ParameterId {
        &self.id
    }

    #[must_use]
    pub const fn domain(&self) -> ParameterDomain {
        self.domain
    }

    #[must_use]
    pub const fn cardinality(&self) -> ArgumentCardinality {
        self.cardinality
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionDefinition {
    key: ActionKey,
    category: ActionCategory,
    title: BoundedText,
    description: BoundedText,
    keywords: Box<[BoundedText]>,
    parameters: Box<[ActionParameter]>,
    permitted_uses: Box<[PermittedUse]>,
    confirmation: ConfirmationPolicy,
    undo: UndoPolicy,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionDefinitionSpec {
    pub key: ActionKey,
    pub category: ActionCategory,
    pub title: BoundedText,
    pub description: BoundedText,
    pub keywords: Vec<BoundedText>,
    pub parameters: Vec<ActionParameter>,
    pub permitted_uses: Vec<PermittedUse>,
    pub confirmation: ConfirmationPolicy,
    pub undo: UndoPolicy,
}

impl ActionDefinition {
    /// Creates one bounded stable action definition.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogContractError`] for repeated parameters or uses,
    /// missing permitted uses, or collections above their protocol bounds.
    pub fn new(mut spec: ActionDefinitionSpec) -> Result<Self, CatalogContractError> {
        bounded(
            "definition keywords",
            spec.keywords.len(),
            MAX_DEFINITION_KEYWORDS,
        )?;
        bounded(
            "definition parameters",
            spec.parameters.len(),
            MAX_DEFINITION_PARAMETERS,
        )?;
        if spec.title.as_str().trim().is_empty() || spec.description.as_str().trim().is_empty() {
            return Err(CatalogContractError::EmptyDefinitionText);
        }
        if has_duplicate(&spec.keywords) {
            return Err(CatalogContractError::DuplicateKeyword);
        }
        if spec.permitted_uses.is_empty() {
            return Err(CatalogContractError::NoPermittedUses);
        }
        let use_count = spec
            .permitted_uses
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .len();
        if use_count != spec.permitted_uses.len() {
            return Err(CatalogContractError::DuplicatePermittedUse);
        }
        spec.permitted_uses.sort_unstable();
        let parameter_count = spec
            .parameters
            .iter()
            .map(ActionParameter::id)
            .collect::<BTreeSet<_>>()
            .len();
        if parameter_count != spec.parameters.len() {
            return Err(CatalogContractError::DuplicateParameter);
        }
        Ok(Self {
            key: spec.key,
            category: spec.category,
            title: spec.title,
            description: spec.description,
            keywords: spec.keywords.into_boxed_slice(),
            parameters: spec.parameters.into_boxed_slice(),
            permitted_uses: spec.permitted_uses.into_boxed_slice(),
            confirmation: spec.confirmation,
            undo: spec.undo,
        })
    }

    #[must_use]
    pub const fn key(&self) -> &ActionKey {
        &self.key
    }

    #[must_use]
    pub const fn category(&self) -> ActionCategory {
        self.category
    }

    #[must_use]
    pub const fn title(&self) -> &BoundedText {
        &self.title
    }

    #[must_use]
    pub const fn description(&self) -> &BoundedText {
        &self.description
    }

    #[must_use]
    pub const fn keywords(&self) -> &[BoundedText] {
        &self.keywords
    }

    #[must_use]
    pub const fn parameters(&self) -> &[ActionParameter] {
        &self.parameters
    }

    #[must_use]
    pub const fn permitted_uses(&self) -> &[PermittedUse] {
        &self.permitted_uses
    }

    #[must_use]
    pub const fn confirmation(&self) -> ConfirmationPolicy {
        self.confirmation
    }

    #[must_use]
    pub const fn undo(&self) -> UndoPolicy {
        self.undo
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActionUnavailability {
    ManagerPaused = 1,
    NoFocusedWindow = 2,
    NoWindowInDirection = 3,
    Unauthorized = 4,
    UnknownWorkspace = 5,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActionAvailability {
    Available,
    Unavailable(ActionUnavailability),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DynamicParameterChoices {
    parameter: ParameterId,
    choices: Box<[ArgumentScalar]>,
}

impl DynamicParameterChoices {
    /// Creates one nonempty bounded dynamic-choice group.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogContractError`] when choices are empty or oversized.
    pub fn new(
        parameter: ParameterId,
        choices: Vec<ArgumentScalar>,
    ) -> Result<Self, CatalogContractError> {
        if choices.is_empty() {
            return Err(CatalogContractError::EmptyDynamicChoices);
        }
        bounded("dynamic choices", choices.len(), MAX_DYNAMIC_CHOICES)?;
        if has_duplicate(&choices) {
            return Err(CatalogContractError::DuplicateDynamicChoice);
        }
        Ok(Self {
            parameter,
            choices: choices.into_boxed_slice(),
        })
    }

    #[must_use]
    pub const fn parameter(&self) -> &ParameterId {
        &self.parameter
    }

    #[must_use]
    pub const fn choices(&self) -> &[ArgumentScalar] {
        &self.choices
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionOffer {
    reference: OfferRef,
    state: StateStamp,
    availability: ActionAvailability,
    current_value: Option<ArgumentScalar>,
    dynamic_choices: Box<[DynamicParameterChoices]>,
    bindings: Box<[BoundedText]>,
}

impl ActionOffer {
    /// Creates one bounded action offer for an exact manager state.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogContractError`] for cross-epoch values or oversized
    /// dynamic choices and binding hints.
    pub fn new(
        reference: OfferRef,
        state: StateStamp,
        availability: ActionAvailability,
        current_value: Option<ArgumentScalar>,
        dynamic_choices: Vec<DynamicParameterChoices>,
        bindings: Vec<BoundedText>,
    ) -> Result<Self, CatalogContractError> {
        bounded(
            "dynamic choice groups",
            dynamic_choices.len(),
            MAX_DYNAMIC_CHOICE_GROUPS,
        )?;
        bounded("binding hints", bindings.len(), MAX_BINDING_HINTS)?;
        if has_duplicate_by(&dynamic_choices, DynamicParameterChoices::parameter) {
            return Err(CatalogContractError::DuplicateDynamicChoiceGroup);
        }
        if has_duplicate(&bindings) {
            return Err(CatalogContractError::DuplicateBindingHint);
        }
        if reference.catalog().epoch() != state.epoch() {
            return Err(CatalogContractError::EpochMismatch);
        }
        Ok(Self {
            reference,
            state,
            availability,
            current_value,
            dynamic_choices: dynamic_choices.into_boxed_slice(),
            bindings: bindings.into_boxed_slice(),
        })
    }

    #[must_use]
    pub const fn reference(&self) -> &OfferRef {
        &self.reference
    }

    #[must_use]
    pub const fn state(&self) -> StateStamp {
        self.state
    }

    #[must_use]
    pub const fn availability(&self) -> ActionAvailability {
        self.availability
    }

    #[must_use]
    pub const fn current_value(&self) -> Option<&ArgumentScalar> {
        self.current_value.as_ref()
    }

    #[must_use]
    pub const fn dynamic_choices(&self) -> &[DynamicParameterChoices] {
        &self.dynamic_choices
    }

    #[must_use]
    pub const fn bindings(&self) -> &[BoundedText] {
        &self.bindings
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogSnapshot {
    stamp: CatalogStamp,
    state: StateStamp,
    definitions: Box<[ActionDefinition]>,
    offers: Box<[ActionOffer]>,
}

impl CatalogSnapshot {
    /// Creates a canonical immutable definition-and-offer snapshot.
    ///
    /// Definitions and offers are sorted by action key before storage.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogContractError`] for oversized, duplicate, unmatched,
    /// or cross-epoch values.
    pub fn new(
        stamp: CatalogStamp,
        state: StateStamp,
        mut definitions: Vec<ActionDefinition>,
        mut offers: Vec<ActionOffer>,
    ) -> Result<Self, CatalogContractError> {
        bounded(
            "catalog definitions",
            definitions.len(),
            MAX_CATALOG_ACTIONS,
        )?;
        bounded("catalog offers", offers.len(), MAX_CATALOG_ACTIONS)?;
        if stamp.epoch() != state.epoch() {
            return Err(CatalogContractError::EpochMismatch);
        }
        if definitions.len() != offers.len() {
            return Err(CatalogContractError::DefinitionOfferCountMismatch);
        }
        definitions.sort_unstable_by(|left, right| left.key().cmp(right.key()));
        offers.sort_unstable_by(|left, right| {
            left.reference().action().cmp(right.reference().action())
        });
        for pair in definitions.windows(2) {
            if pair[0].key() == pair[1].key() {
                return Err(CatalogContractError::DuplicateAction);
            }
        }
        for (definition, offer) in definitions.iter().zip(&offers) {
            if offer.reference().catalog() != stamp
                || offer.state() != state
                || offer.reference().action() != definition.key()
            {
                return Err(CatalogContractError::OfferOutsideSnapshot);
            }
        }
        Ok(Self {
            stamp,
            state,
            definitions: definitions.into_boxed_slice(),
            offers: offers.into_boxed_slice(),
        })
    }

    #[must_use]
    pub const fn stamp(&self) -> CatalogStamp {
        self.stamp
    }

    #[must_use]
    pub const fn state(&self) -> StateStamp {
        self.state
    }

    #[must_use]
    pub const fn definitions(&self) -> &[ActionDefinition] {
        &self.definitions
    }

    #[must_use]
    pub const fn offers(&self) -> &[ActionOffer] {
        &self.offers
    }
}
