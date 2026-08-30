use std::num::NonZeroU16;

use komorebi_protocol as protocol;
use thiserror::Error;

use crate::action;
use crate::action::definition;
use crate::action::offer;
use crate::core::OperationDirection;

#[must_use]
pub fn action_grants(authority: &protocol::AuthoritySummary) -> action::ActionGrants {
    if authority.permits(protocol::CommandCapability::InvokeActions) {
        action::ActionGrants::all()
    } else {
        action::ActionGrants::none()
    }
}

/// Projects the manager's typed action model into one immutable wire catalog.
///
/// # Errors
///
/// Returns [`CatalogProjectionError`] if an internal definition violates the
/// bounded public protocol or its stable identifiers cannot be represented.
pub fn snapshot(
    observation: &action::ActionSnapshot,
    authority: &action::ActionAuthority,
) -> Result<protocol::CatalogSnapshot, CatalogProjectionError> {
    let stamp = protocol::CatalogStamp::new(
        observation.state.epoch(),
        protocol::Revision::FIRST,
        observation.state.revision(),
        authority.grants.revision(),
    );
    let internal_offers = offer::offers(observation, authority);
    let mut definitions = Vec::with_capacity(internal_offers.len());
    let mut offers = Vec::with_capacity(internal_offers.len());
    for internal_offer in internal_offers {
        let definition = project_definition(internal_offer.definition)?;
        let fingerprint = protocol::CatalogCodec::definition_fingerprint(&definition)?;
        let reference = protocol::OfferRef::new(definition.key().clone(), fingerprint, stamp);
        offers.push(project_offer(reference, &internal_offer)?);
        definitions.push(definition);
    }
    Ok(protocol::CatalogSnapshot::new(
        stamp,
        observation.state,
        definitions,
        offers,
    )?)
}

/// Returns an exact-stamp cache result without weakening snapshot identity.
///
/// # Errors
///
/// Returns [`CatalogProjectionError`] when the current manager catalog cannot
/// be represented by the public protocol.
pub fn reply(
    observation: &action::ActionSnapshot,
    authority: &action::ActionAuthority,
    known: Option<protocol::CatalogStamp>,
) -> Result<protocol::CatalogReply, CatalogProjectionError> {
    let snapshot = snapshot(observation, authority)?;
    if known == Some(snapshot.stamp()) {
        Ok(protocol::CatalogReply::NotModified(snapshot.stamp()))
    } else {
        Ok(protocol::CatalogReply::Snapshot(snapshot))
    }
}

fn project_definition(
    value: &action::ActionDefinition,
) -> Result<protocol::ActionDefinition, CatalogProjectionError> {
    let schema_version = NonZeroU16::new(value.schema_version.get())
        .ok_or(CatalogProjectionError::ZeroSchemaVersion)?;
    Ok(protocol::ActionDefinition::new(
        protocol::ActionDefinitionSpec {
            key: protocol::ActionKey::new(
                protocol::ActionId::parse(value.id.as_str())?,
                protocol::ActionSchemaVersion::new(schema_version),
            ),
            category: project_category(value.category),
            title: protocol::BoundedText::new(value.title)?,
            description: protocol::BoundedText::new(value.description)?,
            keywords: value
                .keywords
                .iter()
                .map(|keyword| protocol::BoundedText::new(*keyword))
                .collect::<Result<_, _>>()?,
            parameters: value
                .parameters
                .iter()
                .map(|parameter| {
                    Ok(protocol::ActionParameter::new(
                        protocol::ParameterId::parse(parameter.id.as_str())?,
                        project_parameter_domain(parameter.domain),
                    ))
                })
                .collect::<Result<_, protocol::StableIdError>>()?,
            permitted_uses: value
                .permitted_uses
                .iter()
                .copied()
                .map(project_permitted_use)
                .collect(),
            confirmation: project_confirmation(value.confirmation),
            undo: project_undo(value.undo),
        },
    )?)
}

fn project_offer(
    reference: protocol::OfferRef,
    value: &action::ActionOffer,
) -> Result<protocol::ActionOffer, CatalogProjectionError> {
    Ok(protocol::ActionOffer::new(
        reference,
        value.state,
        project_availability(&value.availability),
        value.current_value.map(project_current_value).transpose()?,
        value
            .dynamic_choices
            .iter()
            .map(project_dynamic_choices)
            .collect::<Result<_, _>>()?,
        value
            .bindings
            .iter()
            .map(|binding| protocol::BoundedText::new(binding.trigger.as_str()))
            .collect::<Result<_, _>>()?,
    )?)
}

fn project_dynamic_choices(
    value: &action::DynamicParameterChoices,
) -> Result<protocol::DynamicParameterChoices, CatalogProjectionError> {
    Ok(protocol::DynamicParameterChoices::new(
        protocol::ParameterId::parse(value.parameter.as_str())?,
        value
            .choices
            .iter()
            .map(project_dynamic_choice)
            .collect::<Result<_, _>>()?,
    )?)
}

fn project_dynamic_choice(
    value: &action::DynamicParameterChoice,
) -> Result<protocol::ArgumentScalar, CatalogProjectionError> {
    match value {
        action::DynamicParameterChoice::Direction(direction) => {
            Ok(protocol::ArgumentScalar::Choice(protocol::ChoiceId::parse(
                direction_name(*direction),
            )?))
        }
        action::DynamicParameterChoice::WorkspaceName(name) => Ok(protocol::ArgumentScalar::Text(
            protocol::BoundedText::new(name.as_str())?,
        )),
    }
}

fn project_current_value(
    value: offer::ActionCurrentValue,
) -> Result<protocol::ArgumentScalar, CatalogProjectionError> {
    match value {
        offer::ActionCurrentValue::Layout(layout) => Ok(protocol::ArgumentScalar::Choice(
            protocol::ChoiceId::parse(definition::layout_name(layout))?,
        )),
        offer::ActionCurrentValue::Floating(floating) => {
            Ok(protocol::ArgumentScalar::Bool(floating))
        }
    }
}

const fn project_category(value: definition::ActionCategory) -> protocol::ActionCategory {
    match value {
        definition::ActionCategory::Window => protocol::ActionCategory::Window,
        definition::ActionCategory::Workspace => protocol::ActionCategory::Workspace,
    }
}

const fn project_permitted_use(value: definition::PermittedUse) -> protocol::PermittedUse {
    match value {
        definition::PermittedUse::Interactive => protocol::PermittedUse::Interactive,
        definition::PermittedUse::Automation => protocol::PermittedUse::Automation,
    }
}

const fn project_confirmation(
    value: definition::ConfirmationPolicy,
) -> protocol::ConfirmationPolicy {
    match value {
        definition::ConfirmationPolicy::None => protocol::ConfirmationPolicy::None,
    }
}

const fn project_undo(value: definition::UndoPolicy) -> protocol::UndoPolicy {
    match value {
        definition::UndoPolicy::None => protocol::UndoPolicy::None,
        definition::UndoPolicy::PriorManagerIntent => protocol::UndoPolicy::PriorManagerIntent,
        definition::UndoPolicy::ExactCapturedState => protocol::UndoPolicy::ExactCapturedState,
    }
}

const fn project_parameter_domain(value: definition::ParameterDomain) -> protocol::ParameterDomain {
    match value {
        definition::ParameterDomain::Direction => protocol::ParameterDomain::Direction,
        definition::ParameterDomain::Axis => protocol::ParameterDomain::Axis,
        definition::ParameterDomain::Pixels => protocol::ParameterDomain::Pixels,
        definition::ParameterDomain::WorkspaceSelector => {
            protocol::ParameterDomain::WorkspaceSelector
        }
        definition::ParameterDomain::WindowSelector => protocol::ParameterDomain::WindowSelector,
        definition::ParameterDomain::Layout => protocol::ParameterDomain::Layout,
        definition::ParameterDomain::Cycle => protocol::ParameterDomain::Cycle,
        definition::ParameterDomain::Index => protocol::ParameterDomain::Index,
        definition::ParameterDomain::Sizing => protocol::ParameterDomain::Sizing,
        definition::ParameterDomain::Adjustment => protocol::ParameterDomain::Adjustment,
        definition::ParameterDomain::Flag => protocol::ParameterDomain::Flag,
        definition::ParameterDomain::Size => protocol::ParameterDomain::Size,
        definition::ParameterDomain::Count => protocol::ParameterDomain::Count,
        definition::ParameterDomain::Columns => protocol::ParameterDomain::Columns,
        definition::ParameterDomain::Name => protocol::ParameterDomain::Name,
        definition::ParameterDomain::Path => protocol::ParameterDomain::Path,
        definition::ParameterDomain::Behaviour => protocol::ParameterDomain::Behaviour,
        definition::ParameterDomain::Implementation => protocol::ParameterDomain::Implementation,
        definition::ParameterDomain::Exe => protocol::ParameterDomain::Executable,
        definition::ParameterDomain::Identifier => protocol::ParameterDomain::Identifier,
        definition::ParameterDomain::Ratios => protocol::ParameterDomain::Ratios,
        definition::ParameterDomain::AtCount => protocol::ParameterDomain::AtCount,
    }
}

const fn project_availability(value: &offer::ActionAvailability) -> protocol::ActionAvailability {
    match value {
        offer::ActionAvailability::Available => protocol::ActionAvailability::Available,
        offer::ActionAvailability::Unavailable(reason) => {
            protocol::ActionAvailability::Unavailable(match reason {
                offer::Unavailability::ManagerPaused => {
                    protocol::ActionUnavailability::ManagerPaused
                }
                offer::Unavailability::NoFocusedWindow => {
                    protocol::ActionUnavailability::NoFocusedWindow
                }
                offer::Unavailability::NoWindowInDirection => {
                    protocol::ActionUnavailability::NoWindowInDirection
                }
                offer::Unavailability::Unauthorized => protocol::ActionUnavailability::Unauthorized,
                offer::Unavailability::UnknownWorkspace => {
                    protocol::ActionUnavailability::UnknownWorkspace
                }
            })
        }
    }
}

const fn direction_name(value: OperationDirection) -> &'static str {
    match value {
        OperationDirection::Left => "left",
        OperationDirection::Right => "right",
        OperationDirection::Up => "up",
        OperationDirection::Down => "down",
    }
}

#[derive(Debug, Error)]
pub enum CatalogProjectionError {
    #[error("internal action schema version must be nonzero")]
    ZeroSchemaVersion,
    #[error(transparent)]
    StableId(#[from] protocol::StableIdError),
    #[error(transparent)]
    Argument(#[from] protocol::ArgumentError),
    #[error(transparent)]
    Catalog(#[from] protocol::CatalogContractError),
    #[error(transparent)]
    Codec(#[from] protocol::CommandCodecError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::ActionGrants;
    use crate::action::ActionSnapshot;
    use komorebi_protocol::ManagerEpoch;

    #[test]
    fn every_builtin_projects_with_a_matching_canonical_fingerprint()
    -> Result<(), Box<dyn std::error::Error>> {
        let epoch = ManagerEpoch::new([4; 16])?;
        let observation = ActionSnapshot::empty(epoch);
        let authority = action::ActionAuthority {
            grants: ActionGrants::all(),
        };
        let projected = snapshot(&observation, &authority)?;
        assert_eq!(
            projected.definitions().len(),
            action::BuiltinActionKind::ALL.len()
        );
        assert_eq!(
            projected.offers().len(),
            action::BuiltinActionKind::ALL.len()
        );
        assert_eq!(projected.state(), observation.state);
        assert_eq!(
            projected.stamp().offer_revision(),
            observation.state.revision()
        );
        for (definition, offer) in projected.definitions().iter().zip(projected.offers()) {
            assert_eq!(definition.key(), offer.reference().action());
            assert_eq!(
                protocol::CatalogCodec::definition_fingerprint(definition)?,
                offer.reference().contract()
            );
        }
        Ok(())
    }

    #[test]
    fn exact_known_stamp_returns_not_modified() -> Result<(), Box<dyn std::error::Error>> {
        let observation = ActionSnapshot::empty(ManagerEpoch::new([5; 16])?);
        let authority = action::ActionAuthority {
            grants: ActionGrants::none(),
        };
        let current = snapshot(&observation, &authority)?;
        assert_eq!(
            reply(&observation, &authority, Some(current.stamp()))?,
            protocol::CatalogReply::NotModified(current.stamp())
        );
        Ok(())
    }

    #[test]
    fn protocol_authority_maps_to_grants_without_client_identity_translation() {
        assert!(
            action_grants(&protocol::AuthoritySummary::command_owner())
                .contains(action::BuiltinActionKind::FocusWindow)
        );
        assert!(
            !action_grants(&protocol::AuthoritySummary::default())
                .contains(action::BuiltinActionKind::FocusWindow)
        );
    }
}
