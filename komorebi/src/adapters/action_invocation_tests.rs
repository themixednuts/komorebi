use std::collections::BTreeMap;

use komorebi_protocol as protocol;

use crate::action::ActionAuthority;
use crate::action::ActionGrants;
use crate::action::ActionSnapshot;
use crate::action::BuiltinAction;
use crate::action::BuiltinActionKind;
use crate::action::DirectionSet;
use crate::action::MonitorIndex;
use crate::action::NamedWorkspaceTarget;
use crate::action::WorkspaceIndex;
use crate::action::WorkspaceName;
use crate::action::id::WindowId;
use crate::core::OperationDirection;

use super::action_catalog;
use super::action_invocation;
use super::action_invocation::InvocationBindingError;

fn catalog() -> Result<protocol::CatalogSnapshot, Box<dyn std::error::Error>> {
    let epoch = protocol::ManagerEpoch::new([7; 16])?;
    let mut snapshot = ActionSnapshot::empty(epoch);
    snapshot.focused_window = Some(WindowId::new(42));
    snapshot.directional_targets = DirectionSet::from([
        OperationDirection::Left,
        OperationDirection::Right,
        OperationDirection::Up,
        OperationDirection::Down,
    ]);
    snapshot.named_workspaces.push(NamedWorkspaceTarget {
        name: WorkspaceName::parse("dev")?,
        monitor: MonitorIndex::new(0),
        workspace: WorkspaceIndex::new(0),
    });
    Ok(action_catalog::snapshot(
        &snapshot,
        &ActionAuthority {
            grants: ActionGrants::all(),
        },
    )?)
}

fn invocation(
    catalog: &protocol::CatalogSnapshot,
    kind: BuiltinActionKind,
    arguments: protocol::ActionArguments,
) -> Result<protocol::ActionInvocation, Box<dyn std::error::Error>> {
    let offer = catalog
        .offers()
        .iter()
        .find(|offer| offer.reference().action().id().as_str() == kind.id().as_str())
        .ok_or("projected offer missing")?;
    Ok(protocol::ActionInvocation::new(
        protocol::InvocationId::new(
            protocol::InvocationNamespaceId::new([8; 16])?,
            protocol::InvocationSequence::try_from(1)?,
        ),
        offer.reference().clone(),
        catalog.state(),
        arguments,
        None,
    ))
}

fn valid_arguments(
    kind: BuiltinActionKind,
) -> Result<protocol::ActionArguments, Box<dyn std::error::Error>> {
    let mut values = BTreeMap::new();
    for parameter in kind.definition().parameters {
        if matches!(
            parameter.cardinality,
            crate::action::definition::ArgumentCardinality::OptionalList
                | crate::action::definition::ArgumentCardinality::OptionalScalar
        ) {
            continue;
        }
        let scalar = scalar(kind, parameter.domain)?;
        let argument = match parameter.cardinality {
            crate::action::definition::ArgumentCardinality::RequiredScalar => {
                protocol::ActionArgument::Scalar(scalar)
            }
            crate::action::definition::ArgumentCardinality::RequiredList => {
                protocol::ActionArgument::Scalars(protocol::ArgumentScalars::new([scalar])?)
            }
            crate::action::definition::ArgumentCardinality::OptionalScalar
            | crate::action::definition::ArgumentCardinality::OptionalList => continue,
        };
        values.insert(
            protocol::ParameterId::parse(parameter.id.as_str())?,
            argument,
        );
    }
    Ok(protocol::ActionArguments::new(values)?)
}

fn scalar(
    kind: BuiltinActionKind,
    domain: crate::action::definition::ParameterDomain,
) -> Result<protocol::ArgumentScalar, Box<dyn std::error::Error>> {
    use crate::action::definition::ParameterDomain as D;
    Ok(match domain {
        D::Direction => protocol::ArgumentScalar::Choice(protocol::ChoiceId::parse("left")?),
        D::Axis => protocol::ArgumentScalar::Choice(protocol::ChoiceId::parse("horizontal")?),
        D::Pixels | D::Adjustment | D::Size | D::ResizeStep => protocol::ArgumentScalar::Signed(1),
        D::WorkspaceSelector | D::WindowSelector => {
            protocol::ArgumentScalar::Selector(protocol::SelectorId::parse("focused-at-execution")?)
        }
        D::Layout => protocol::ArgumentScalar::Choice(protocol::ChoiceId::parse("bsp")?),
        D::Cycle => protocol::ArgumentScalar::Choice(protocol::ChoiceId::parse("next")?),
        D::Index | D::Count | D::Columns | D::AtCount => protocol::ArgumentScalar::Unsigned(1),
        D::Sizing => protocol::ArgumentScalar::Choice(protocol::ChoiceId::parse("increase")?),
        D::Flag => protocol::ArgumentScalar::Bool(true),
        D::Name | D::Exe => protocol::ArgumentScalar::Text(protocol::BoundedText::new("dev")?),
        D::Path => protocol::ArgumentScalar::WindowsPath(protocol::WindowsPathInput::new([
            b'C' as u16,
            b':' as u16,
            b'\\' as u16,
            0xD800,
            b'x' as u16,
        ])?),
        D::Behaviour => protocol::ArgumentScalar::Choice(protocol::ChoiceId::parse(match kind {
            BuiltinActionKind::SetWindowHidingBehaviour => "cloak",
            BuiltinActionKind::SetCrossMonitorMoveBehaviour => "swap",
            BuiltinActionKind::SetMonocleFocusBehaviour => "cycle",
            BuiltinActionKind::SetUnmanagedWindowOperationBehaviour => "op",
            _ => return Err("unexpected behaviour domain".into()),
        })?),
        D::Implementation => {
            protocol::ArgumentScalar::Choice(protocol::ChoiceId::parse("komorebi")?)
        }
        D::Identifier => protocol::ArgumentScalar::Choice(protocol::ChoiceId::parse("exe")?),
        D::Ratios => protocol::ArgumentScalar::Decimal(protocol::FixedDecimal::new(1, 1)?),
        D::Alpha => protocol::ArgumentScalar::Unsigned(200),
    })
}

#[test]
fn every_advertised_builtin_binds_to_its_closed_manager_variant()
-> Result<(), Box<dyn std::error::Error>> {
    let catalog = catalog()?;
    for kind in BuiltinActionKind::ALL {
        let request = invocation(&catalog, kind, valid_arguments(kind)?)?;
        let bound = action_invocation::bind(&catalog, &request)
            .map_err(|error| format!("{kind:?} did not bind: {error}"))?;
        assert_eq!(bound.action.kind(), kind);
    }
    Ok(())
}

#[test]
fn exact_cardinality_and_unknown_arguments_fail_closed() -> Result<(), Box<dyn std::error::Error>> {
    let catalog = catalog()?;
    let mut values = BTreeMap::new();
    values.insert(
        protocol::ParameterId::parse("direction")?,
        protocol::ActionArgument::Scalars(protocol::ArgumentScalars::new([
            protocol::ArgumentScalar::Choice(protocol::ChoiceId::parse("left")?),
        ])?),
    );
    let request = invocation(
        &catalog,
        BuiltinActionKind::FocusWindow,
        protocol::ActionArguments::new(values)?,
    )?;
    assert!(matches!(
        action_invocation::bind(&catalog, &request),
        Err(InvocationBindingError::Arguments(_))
    ));

    let mut values = BTreeMap::new();
    values.insert(
        protocol::ParameterId::parse("surprise")?,
        protocol::ActionArgument::Scalar(protocol::ArgumentScalar::Bool(true)),
    );
    let request = invocation(
        &catalog,
        BuiltinActionKind::TogglePause,
        protocol::ActionArguments::new(values)?,
    )?;
    assert!(matches!(
        action_invocation::bind(&catalog, &request),
        Err(InvocationBindingError::Arguments(_))
    ));
    Ok(())
}

#[test]
fn transparency_alpha_rejects_unsigned_values_outside_the_byte_domain()
-> Result<(), Box<dyn std::error::Error>> {
    let catalog = catalog()?;
    let arguments = protocol::ActionArguments::new(BTreeMap::from([(
        protocol::ParameterId::parse("alpha")?,
        protocol::ActionArgument::Scalar(protocol::ArgumentScalar::Unsigned(256)),
    )]))?;
    let request = invocation(&catalog, BuiltinActionKind::SetTransparencyAlpha, arguments)?;

    assert!(matches!(
        action_invocation::bind(&catalog, &request),
        Err(InvocationBindingError::Arguments(
            action_invocation::ArgumentBindingError::OutsideU8 { .. }
        ))
    ));
    Ok(())
}

#[test]
fn stale_state_is_rejected_before_argument_binding() -> Result<(), Box<dyn std::error::Error>> {
    let catalog = catalog()?;
    let current = catalog.state();
    let stale = protocol::StateStamp::new(current.epoch(), current.revision().next()?);
    let offer = catalog
        .offers()
        .iter()
        .find(|offer| offer.reference().action().id().as_str() == "toggle-pause")
        .ok_or("toggle-pause offer missing")?;
    let request = protocol::ActionInvocation::new(
        protocol::InvocationId::new(
            protocol::InvocationNamespaceId::new([9; 16])?,
            protocol::InvocationSequence::try_from(1)?,
        ),
        offer.reference().clone(),
        stale,
        protocol::ActionArguments::default(),
        None,
    );
    assert!(matches!(
        action_invocation::bind(&catalog, &request),
        Err(InvocationBindingError::Rejected(
            protocol::InvocationRejection::StaleState { current: actual }
        )) if actual == current
    ));
    Ok(())
}

#[test]
fn forged_contracts_and_outdated_dynamic_choices_are_stale_offers()
-> Result<(), Box<dyn std::error::Error>> {
    let catalog = catalog()?;
    let offer = catalog
        .offers()
        .iter()
        .find(|offer| offer.reference().action().id().as_str() == "focus-window")
        .ok_or("focus-window offer missing")?;
    let forged = protocol::ActionInvocation::new(
        protocol::InvocationId::new(
            protocol::InvocationNamespaceId::new([10; 16])?,
            protocol::InvocationSequence::try_from(1)?,
        ),
        protocol::OfferRef::new(
            offer.reference().action().clone(),
            protocol::ActionContractFingerprint::new([99; 32]),
            catalog.stamp(),
        ),
        catalog.state(),
        valid_arguments(BuiltinActionKind::FocusWindow)?,
        None,
    );
    assert!(matches!(
        action_invocation::bind(&catalog, &forged),
        Err(InvocationBindingError::Rejected(
            protocol::InvocationRejection::StaleOffer
        ))
    ));

    let mut values = BTreeMap::new();
    values.insert(
        protocol::ParameterId::parse("direction")?,
        protocol::ActionArgument::Scalar(protocol::ArgumentScalar::Choice(
            protocol::ChoiceId::parse("diagonal")?,
        )),
    );
    let outdated = invocation(
        &catalog,
        BuiltinActionKind::FocusWindow,
        protocol::ActionArguments::new(values)?,
    )?;
    assert!(matches!(
        action_invocation::bind(&catalog, &outdated),
        Err(InvocationBindingError::Rejected(
            protocol::InvocationRejection::StaleOffer
        ))
    ));
    Ok(())
}

#[test]
fn wtf16_paths_reach_the_typed_action_without_repair() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::windows::ffi::OsStrExt as _;

    let catalog = catalog()?;
    let request = invocation(
        &catalog,
        BuiltinActionKind::SetCustomLayout,
        valid_arguments(BuiltinActionKind::SetCustomLayout)?,
    )?;
    let bound = action_invocation::bind(&catalog, &request)?;
    let BuiltinAction::SetCustomLayout { path } = bound.action else {
        return Err("custom layout bound to wrong action".into());
    };
    assert_eq!(
        path.as_path().as_os_str().encode_wide().collect::<Vec<_>>(),
        [b'C' as u16, b':' as u16, b'\\' as u16, 0xD800, b'x' as u16]
    );
    Ok(())
}

#[test]
fn layout_ratios_are_accepted_exactly_or_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let catalog = catalog()?;
    let ratios = |coefficients: &[i64]| -> Result<_, Box<dyn std::error::Error>> {
        Ok(protocol::ActionArgument::Scalars(
            protocol::ArgumentScalars::new(
                coefficients
                    .iter()
                    .map(|coefficient| {
                        protocol::FixedDecimal::new(*coefficient, 2)
                            .map(protocol::ArgumentScalar::Decimal)
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            )?,
        ))
    };
    let arguments = |value| -> Result<_, Box<dyn std::error::Error>> {
        let mut values = BTreeMap::new();
        values.insert(protocol::ParameterId::parse("column-ratios")?, value);
        Ok(protocol::ActionArguments::new(values)?)
    };

    let valid = invocation(
        &catalog,
        BuiltinActionKind::SetLayoutRatios,
        arguments(ratios(&[25, 25])?)?,
    )?;
    let bound = action_invocation::bind(&catalog, &valid)?;
    assert!(matches!(
        bound.action,
        BuiltinAction::SetLayoutRatios {
            columns: Some(ref ratios),
            rows: None,
        } if ratios == &[0.25, 0.25]
    ));

    for coefficients in [&[5_i64][..], &[60_i64, 50][..], &[10_i64; 6][..]] {
        let invalid = invocation(
            &catalog,
            BuiltinActionKind::SetLayoutRatios,
            arguments(ratios(coefficients)?)?,
        )?;
        assert!(matches!(
            action_invocation::bind(&catalog, &invalid),
            Err(InvocationBindingError::Arguments(
                action_invocation::ArgumentBindingError::InvalidRatio { .. }
            ))
        ));
    }
    Ok(())
}
