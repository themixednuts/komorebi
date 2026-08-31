use komorebi_protocol::ActionArgument;
use komorebi_protocol::ActionArguments;
use komorebi_protocol::ActionAvailability;
use komorebi_protocol::ActionDefinition;
use komorebi_protocol::ActionId;
use komorebi_protocol::ActionIntent;
use komorebi_protocol::ActionKey;
use komorebi_protocol::ActionOffer;
use komorebi_protocol::ArgumentCardinality;
use komorebi_protocol::ArgumentScalar;
use komorebi_protocol::CatalogSnapshot;
use komorebi_protocol::ParameterDomain;
use komorebi_protocol::ParameterId;

use crate::ActionBindingError;

/// A catalog-bound action ready for exact command submission.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundAction {
    action: ActionKey,
    arguments: ActionArguments,
}

impl BoundAction {
    pub(crate) const fn new(action: ActionKey, arguments: ActionArguments) -> Self {
        Self { action, arguments }
    }

    /// Resolves one protocol-typed intent against an immutable catalog snapshot.
    pub fn from_intent(
        intent: ActionIntent,
        catalog: &CatalogSnapshot,
    ) -> Result<Self, ActionBindingError> {
        let (action, arguments) = intent.into_parts();
        let (definition, offer) = offered_action(catalog, &action)?;

        for supplied in arguments.values().keys() {
            if !definition
                .parameters()
                .iter()
                .any(|parameter| parameter.id() == supplied)
            {
                return Err(ActionBindingError::UnknownParameter(supplied.clone()));
            }
        }

        for parameter in definition.parameters() {
            let Some(argument) = arguments.values().get(parameter.id()) else {
                if matches!(
                    parameter.cardinality(),
                    ArgumentCardinality::RequiredScalar | ArgumentCardinality::RequiredList
                ) {
                    return Err(ActionBindingError::MissingParameter(parameter.id().clone()));
                }
                continue;
            };
            validate_typed_argument(
                parameter.id(),
                parameter.domain(),
                parameter.cardinality(),
                argument,
            )?;
            validate_dynamic_choices(offer, parameter.id(), argument)?;
        }

        Ok(Self::new(definition.key().clone(), arguments))
    }

    #[must_use]
    pub const fn action(&self) -> &ActionKey {
        &self.action
    }

    #[must_use]
    pub const fn arguments(&self) -> &ActionArguments {
        &self.arguments
    }

    #[must_use]
    pub fn into_parts(self) -> (ActionKey, ActionArguments) {
        (self.action, self.arguments)
    }
}

pub(crate) fn offered_action<'catalog>(
    catalog: &'catalog CatalogSnapshot,
    action: &ActionId,
) -> Result<(&'catalog ActionDefinition, &'catalog ActionOffer), ActionBindingError> {
    let mut matches = catalog
        .definitions()
        .iter()
        .enumerate()
        .filter(|(_, definition)| definition.key().id() == action);
    let Some((index, definition)) = matches.next() else {
        return Err(ActionBindingError::ActionNotOffered(action.clone()));
    };
    if matches.next().is_some() {
        return Err(ActionBindingError::AmbiguousAction(action.clone()));
    }
    let offer = catalog
        .offers()
        .get(index)
        .ok_or_else(|| ActionBindingError::ActionNotOffered(action.clone()))?;
    if let ActionAvailability::Unavailable(reason) = offer.availability() {
        return Err(ActionBindingError::Unavailable(reason));
    }
    Ok((definition, offer))
}

pub(crate) fn validate_dynamic_choices(
    offer: &ActionOffer,
    parameter: &ParameterId,
    argument: &ActionArgument,
) -> Result<(), ActionBindingError> {
    let Some(choices) = offer
        .dynamic_choices()
        .iter()
        .find(|choices| choices.parameter() == parameter)
    else {
        return Ok(());
    };
    let supplied = match argument {
        ActionArgument::Scalar(value) => std::slice::from_ref(value),
        ActionArgument::Scalars(values) => values.values(),
    };
    if supplied
        .iter()
        .any(|value| !choices.choices().contains(value))
    {
        Err(ActionBindingError::DynamicChoiceRejected(parameter.clone()))
    } else {
        Ok(())
    }
}

fn validate_typed_argument(
    parameter: &ParameterId,
    domain: ParameterDomain,
    cardinality: ArgumentCardinality,
    argument: &ActionArgument,
) -> Result<(), ActionBindingError> {
    let values = match (cardinality, argument) {
        (
            ArgumentCardinality::RequiredScalar | ArgumentCardinality::OptionalScalar,
            ActionArgument::Scalar(value),
        ) => std::slice::from_ref(value),
        (
            ArgumentCardinality::RequiredList | ArgumentCardinality::OptionalList,
            ActionArgument::Scalars(values),
        ) => values.values(),
        (
            ArgumentCardinality::RequiredScalar | ArgumentCardinality::OptionalScalar,
            ActionArgument::Scalars(_),
        ) => return Err(ActionBindingError::ExpectedScalar(parameter.clone())),
        (
            ArgumentCardinality::RequiredList | ArgumentCardinality::OptionalList,
            ActionArgument::Scalar(_),
        ) => return Err(ActionBindingError::ExpectedList(parameter.clone())),
    };
    let list_item = matches!(
        cardinality,
        ArgumentCardinality::RequiredList | ArgumentCardinality::OptionalList
    );
    if values
        .iter()
        .all(|value| scalar_matches_domain(value, domain, list_item))
    {
        Ok(())
    } else {
        Err(crate::action_input::domain_mismatch(parameter, domain))
    }
}

fn scalar_matches_domain(value: &ArgumentScalar, domain: ParameterDomain, list_item: bool) -> bool {
    use ArgumentScalar as S;
    use ParameterDomain as D;

    match domain {
        D::Flag => matches!(value, S::Bool(_)),
        D::Pixels
        | D::Adjustment
        | D::Size
        | D::ResizeStep
        | D::BorderWidth
        | D::BorderOffset
        | D::StackbarHeight
        | D::StackbarTabWidth
        | D::StackbarFontSize
        | D::WorkAreaOffset => matches!(value, S::Signed(_)),
        D::Index
        | D::Count
        | D::Columns
        | D::AtCount
        | D::Alpha
        | D::ColourChannel
        | D::AnimationDuration
        | D::AnimationFps => matches!(value, S::Unsigned(_)),
        D::Name | D::Executable | D::StackbarFontFamily => matches!(value, S::Text(_)),
        D::Path => matches!(value, S::WindowsPath(_)),
        D::WorkspaceSelector | D::WindowSelector => matches!(value, S::Selector(_)),
        D::Ratios => matches!(value, S::Decimal(_)),
        D::AnimationStyle if list_item => matches!(value, S::Decimal(_)),
        D::Direction
        | D::Axis
        | D::Layout
        | D::Cycle
        | D::Sizing
        | D::Behaviour
        | D::Implementation
        | D::Identifier
        | D::WindowKind
        | D::BorderStyle
        | D::BorderImplementation
        | D::StackbarMode
        | D::StackbarLabel
        | D::AnimationPrefix
        | D::AnimationStyle
        | D::CursorWarpPolicy
        | D::WorkspaceTarget => matches!(value, S::Choice(_)),
    }
}
