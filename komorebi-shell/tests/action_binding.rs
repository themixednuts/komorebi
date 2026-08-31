use std::collections::BTreeMap;
use std::num::NonZeroU16;

use komorebi_protocol::ActionArgument;
use komorebi_protocol::ActionAvailability;
use komorebi_protocol::ActionCategory;
use komorebi_protocol::ActionDefinition;
use komorebi_protocol::ActionDefinitionSpec;
use komorebi_protocol::ActionId;
use komorebi_protocol::ActionIntent;
use komorebi_protocol::ActionKey;
use komorebi_protocol::ActionOffer;
use komorebi_protocol::ActionParameter;
use komorebi_protocol::ActionSchemaVersion;
use komorebi_protocol::ArgumentCardinality;
use komorebi_protocol::ArgumentScalar;
use komorebi_protocol::BoundedText;
use komorebi_protocol::CatalogCodec;
use komorebi_protocol::CatalogSnapshot;
use komorebi_protocol::CatalogStamp;
use komorebi_protocol::ChoiceId;
use komorebi_protocol::ConfirmationPolicy;
use komorebi_protocol::DynamicParameterChoices;
use komorebi_protocol::ManagerEpoch;
use komorebi_protocol::OfferRef;
use komorebi_protocol::ParameterDomain;
use komorebi_protocol::ParameterId;
use komorebi_protocol::PermittedUse;
use komorebi_protocol::Revision;
use komorebi_protocol::StateStamp;
use komorebi_protocol::UndoPolicy;
use komorebi_shell::ActionBinding;
use komorebi_shell::ActionBindingError;
use komorebi_shell::ActionInput;
use komorebi_shell::ActionInputScalar;
use komorebi_shell::BoundAction;

fn snapshot(
    action_id: &str,
    parameter_id: &str,
    domain: ParameterDomain,
    cardinality: ArgumentCardinality,
    choices: Vec<ArgumentScalar>,
) -> Result<CatalogSnapshot, Box<dyn std::error::Error>> {
    let epoch = ManagerEpoch::new([1; 16])?;
    let state = StateStamp::new(epoch, Revision::try_from(7)?);
    let stamp = CatalogStamp::new(
        epoch,
        Revision::try_from(2)?,
        Revision::try_from(7)?,
        Revision::FIRST,
    );
    let definition = ActionDefinition::new(ActionDefinitionSpec {
        key: ActionKey::new(
            ActionId::parse(action_id.to_owned())?,
            ActionSchemaVersion::new(NonZeroU16::MIN),
        ),
        category: ActionCategory::Window,
        title: BoundedText::new("Focus window")?,
        description: BoundedText::new("Focus the neighboring window")?,
        keywords: vec![],
        parameters: vec![ActionParameter::new(
            ParameterId::parse(parameter_id.to_owned())?,
            domain,
            cardinality,
        )],
        permitted_uses: vec![PermittedUse::Interactive],
        confirmation: ConfirmationPolicy::None,
        undo: UndoPolicy::None,
    })?;
    let fingerprint = CatalogCodec::definition_fingerprint(&definition)?;
    let offer = ActionOffer::new(
        OfferRef::new(definition.key().clone(), fingerprint, stamp),
        state,
        ActionAvailability::Available,
        None,
        if choices.is_empty() {
            vec![]
        } else {
            vec![DynamicParameterChoices::new(
                ParameterId::parse(parameter_id.to_owned())?,
                choices,
            )?]
        },
        vec![],
    )?;
    Ok(CatalogSnapshot::new(
        stamp,
        state,
        vec![definition],
        vec![offer],
    )?)
}

#[test]
fn json_binding_resolves_to_the_exact_catalog_action_and_argument_shape()
-> Result<(), Box<dyn std::error::Error>> {
    let binding: ActionBinding = serde_json::from_value(serde_json::json!({
        "action": "focus-window",
        "arguments": { "direction": "left" }
    }))?;

    let bound = binding.bind(&snapshot(
        "focus-window",
        "direction",
        ParameterDomain::Direction,
        ArgumentCardinality::RequiredScalar,
        vec![
            ArgumentScalar::Choice(ChoiceId::parse("left")?),
            ArgumentScalar::Choice(ChoiceId::parse("right")?),
        ],
    )?)?;

    assert_eq!(bound.action().id().as_str(), "focus-window");
    assert_eq!(bound.action().schema_version().get(), 1);
    assert!(matches!(
        bound
            .arguments()
            .values()
            .get(&ParameterId::parse("direction")?),
        Some(ActionArgument::Scalar(ArgumentScalar::Choice(choice)))
            if choice.as_str() == "left"
    ));
    Ok(())
}

#[test]
fn native_windows_path_input_preserves_unpaired_utf16_units()
-> Result<(), Box<dyn std::error::Error>> {
    let units = [
        u16::from(b'C'),
        u16::from(b':'),
        u16::from(b'\\'),
        0xD800,
        u16::from(b'x'),
    ];
    let binding = ActionBinding::new(
        ActionId::parse("load-layout")?,
        BTreeMap::from([(
            ParameterId::parse("path")?,
            ActionInput::from(ActionInputScalar::windows_path_units(units)?),
        )]),
    );

    let bound = binding.bind(&snapshot(
        "load-layout",
        "path",
        ParameterDomain::Path,
        ArgumentCardinality::RequiredScalar,
        vec![],
    )?)?;

    assert!(matches!(
        bound
            .arguments()
            .values()
            .get(&ParameterId::parse("path")?),
        Some(ActionArgument::Scalar(ArgumentScalar::WindowsPath(path)))
            if path.units() == units
    ));
    assert!(serde_json::to_string(&binding).is_err());
    Ok(())
}

#[test]
fn binding_rejects_missing_unknown_and_out_of_offer_values()
-> Result<(), Box<dyn std::error::Error>> {
    let catalog = snapshot(
        "focus-window",
        "direction",
        ParameterDomain::Direction,
        ArgumentCardinality::RequiredScalar,
        vec![ArgumentScalar::Choice(ChoiceId::parse("left")?)],
    )?;

    let missing: ActionBinding = serde_json::from_value(serde_json::json!({
        "action": "focus-window"
    }))?;
    assert!(matches!(
        missing.bind(&catalog),
        Err(ActionBindingError::MissingParameter(parameter))
            if parameter.as_str() == "direction"
    ));

    let unknown: ActionBinding = serde_json::from_value(serde_json::json!({
        "action": "focus-window",
        "arguments": { "side": "left" }
    }))?;
    assert!(matches!(
        unknown.bind(&catalog),
        Err(ActionBindingError::UnknownParameter(parameter))
            if parameter.as_str() == "side"
    ));

    let rejected: ActionBinding = serde_json::from_value(serde_json::json!({
        "action": "focus-window",
        "arguments": { "direction": "right" }
    }))?;
    assert!(matches!(
        rejected.bind(&catalog),
        Err(ActionBindingError::DynamicChoiceRejected(parameter))
            if parameter.as_str() == "direction"
    ));
    Ok(())
}

#[test]
fn domain_mismatch_identifies_the_failing_parameter() -> Result<(), Box<dyn std::error::Error>> {
    let binding: ActionBinding = serde_json::from_value(serde_json::json!({
        "action": "focus-window",
        "arguments": { "direction": true }
    }))?;

    assert!(matches!(
        binding.bind(&snapshot(
            "focus-window",
            "direction",
            ParameterDomain::Direction,
            ArgumentCardinality::RequiredScalar,
            vec![],
        )?),
        Err(ActionBindingError::InputDomainMismatch { parameter, domain })
            if parameter.as_str() == "direction" && domain == ParameterDomain::Direction
    ));
    Ok(())
}

#[test]
fn typed_intent_binds_without_reinterpreting_wtf16_or_choice_values()
-> Result<(), Box<dyn std::error::Error>> {
    let intent = ActionIntent::new(
        ActionId::parse("focus-window")?,
        komorebi_protocol::ActionArguments::new(BTreeMap::from([(
            ParameterId::parse("direction")?,
            ActionArgument::Scalar(ArgumentScalar::Choice(ChoiceId::parse("left")?)),
        )]))?,
    );

    let bound = BoundAction::from_intent(
        intent,
        &snapshot(
            "focus-window",
            "direction",
            ParameterDomain::Direction,
            ArgumentCardinality::RequiredScalar,
            vec![ArgumentScalar::Choice(ChoiceId::parse("left")?)],
        )?,
    )?;

    assert_eq!(bound.action().id().as_str(), "focus-window");
    assert!(matches!(
        bound
            .arguments()
            .values()
            .get(&ParameterId::parse("direction")?),
        Some(ActionArgument::Scalar(ArgumentScalar::Choice(choice)))
            if choice.as_str() == "left"
    ));
    Ok(())
}

#[test]
fn typed_intent_rejects_a_protocol_value_outside_the_catalog_domain()
-> Result<(), Box<dyn std::error::Error>> {
    let intent = ActionIntent::new(
        ActionId::parse("focus-window")?,
        komorebi_protocol::ActionArguments::new(BTreeMap::from([(
            ParameterId::parse("direction")?,
            ActionArgument::Scalar(ArgumentScalar::Bool(true)),
        )]))?,
    );

    assert!(matches!(
        BoundAction::from_intent(
            intent,
            &snapshot(
                "focus-window",
                "direction",
                ParameterDomain::Direction,
                ArgumentCardinality::RequiredScalar,
                vec![],
            )?,
        ),
        Err(ActionBindingError::InputDomainMismatch { parameter, domain })
            if parameter.as_str() == "direction" && domain == ParameterDomain::Direction
    ));
    Ok(())
}
