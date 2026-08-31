use std::num::NonZeroU16;

use komorebi_protocol::ActionAvailability;
use komorebi_protocol::ActionCategory;
use komorebi_protocol::ActionDefinition;
use komorebi_protocol::ActionDefinitionSpec;
use komorebi_protocol::ActionId;
use komorebi_protocol::ActionKey;
use komorebi_protocol::ActionOffer;
use komorebi_protocol::ActionParameter;
use komorebi_protocol::ActionSchemaVersion;
use komorebi_protocol::ActionUnavailability;
use komorebi_protocol::ArgumentCardinality;
use komorebi_protocol::BoundedText;
use komorebi_protocol::CatalogCodec;
use komorebi_protocol::CatalogSnapshot;
use komorebi_protocol::CatalogStamp;
use komorebi_protocol::ConfirmationPolicy;
use komorebi_protocol::ManagerEpoch;
use komorebi_protocol::OfferRef;
use komorebi_protocol::ParameterDomain;
use komorebi_protocol::ParameterId;
use komorebi_protocol::PermittedUse;
use komorebi_protocol::Revision;
use komorebi_protocol::StateStamp;
use komorebi_protocol::UndoPolicy;
use komorebi_shell::CommandPalette;
use komorebi_shell::PaletteActionState;
use komorebi_shell::PaletteSelectionMove;

fn definition(
    id: &str,
    title: &str,
    description: &str,
    keywords: &[&str],
    parameters: Vec<ActionParameter>,
) -> Result<ActionDefinition, Box<dyn std::error::Error>> {
    ActionDefinition::new(ActionDefinitionSpec {
        key: ActionKey::new(
            ActionId::parse(id)?,
            ActionSchemaVersion::new(NonZeroU16::MIN),
        ),
        category: ActionCategory::Window,
        title: BoundedText::new(title)?,
        description: BoundedText::new(description)?,
        keywords: keywords
            .iter()
            .map(|keyword| BoundedText::new(*keyword))
            .collect::<Result<_, _>>()?,
        parameters,
        permitted_uses: vec![PermittedUse::Interactive],
        confirmation: ConfirmationPolicy::None,
        undo: UndoPolicy::None,
    })
    .map_err(Into::into)
}

fn catalog() -> Result<CatalogSnapshot, Box<dyn std::error::Error>> {
    let epoch = ManagerEpoch::new([91; 16])?;
    let revision = Revision::FIRST;
    let stamp = CatalogStamp::new(epoch, revision, revision, revision);
    let state = StateStamp::new(epoch, revision);
    let definitions = vec![
        definition(
            "close-window",
            "Close window",
            "Close the focused window",
            &["quit"],
            vec![],
        )?,
        definition(
            "focus-window",
            "Focus window",
            "Focus the neighboring window",
            &["navigation", "neighbor"],
            vec![],
        )?,
        definition(
            "move-window",
            "Move window",
            "Move the focused window in one direction",
            &["send", "direction"],
            vec![ActionParameter::new(
                ParameterId::parse("direction")?,
                ParameterDomain::Direction,
                ArgumentCardinality::RequiredScalar,
            )],
        )?,
    ];
    let offers = definitions
        .iter()
        .map(|definition| {
            let availability = if definition.key().id().as_str() == "close-window" {
                ActionAvailability::Unavailable(ActionUnavailability::NoFocusedWindow)
            } else {
                ActionAvailability::Available
            };
            Ok(ActionOffer::new(
                OfferRef::new(
                    definition.key().clone(),
                    CatalogCodec::definition_fingerprint(definition)?,
                    stamp,
                ),
                state,
                availability,
                None,
                vec![],
                vec![],
            )?)
        })
        .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
    Ok(CatalogSnapshot::new(stamp, state, definitions, offers)?)
}

#[test]
fn palette_ranks_catalog_actions_and_exposes_typed_selection_states()
-> Result<(), Box<dyn std::error::Error>> {
    let palette = CommandPalette::project(&catalog()?);

    assert_eq!(palette.actions().len(), 3);
    assert_eq!(
        palette
            .search("foc windw")
            .selected(&palette)
            .ok_or("focus search should match")?
            .action_id(),
        "focus-window"
    );
    assert_eq!(
        palette
            .search("neigbor")
            .selected(&palette)
            .ok_or("neighbor search should match")?
            .title(),
        "Focus window"
    );

    let focus_results = palette.search("focus");
    let focus = focus_results
        .selected(&palette)
        .ok_or("focus search should match")?;
    let PaletteActionState::Ready(binding) = focus.state() else {
        return Err("focus-window should be immediately invokable".into());
    };
    assert_eq!(binding.action().as_str(), "focus-window");

    let close_results = palette.search("close");
    let close = close_results
        .selected(&palette)
        .ok_or("close search should match")?;
    assert_eq!(
        close.state(),
        PaletteActionState::Unavailable(ActionUnavailability::NoFocusedWindow)
    );

    let move_results = palette.search("move");
    let move_window = move_results
        .selected(&palette)
        .ok_or("move search should match")?;
    let PaletteActionState::RequiresInput(parameters) = move_window.state() else {
        return Err("move-window should require its direction".into());
    };
    assert_eq!(parameters.len(), 1);
    assert_eq!(parameters[0].id().as_str(), "direction");
    Ok(())
}

#[test]
fn palette_results_own_bounded_wraparound_selection() -> Result<(), Box<dyn std::error::Error>> {
    let palette = CommandPalette::project(&catalog()?);
    let mut results = palette.search("");

    assert_eq!(
        results
            .selected(&palette)
            .ok_or("all actions should select the first result")?
            .action_id(),
        "close-window"
    );
    results.move_selection(PaletteSelectionMove::Next);
    assert_eq!(
        results
            .selected(&palette)
            .ok_or("next action should remain selected")?
            .action_id(),
        "focus-window"
    );
    results.move_selection(PaletteSelectionMove::Previous);
    results.move_selection(PaletteSelectionMove::Previous);
    assert_eq!(
        results
            .selected(&palette)
            .ok_or("previous should wrap to the last action")?
            .action_id(),
        "move-window"
    );
    assert!(!results.select_position(3));
    assert!(results.select_position(1));
    assert_eq!(
        results
            .selected(&palette)
            .ok_or("an in-range row should be selectable")?
            .action_id(),
        "focus-window"
    );

    let mut empty = palette.search("this cannot match any catalog action");
    assert!(empty.selected(&palette).is_none());
    empty.move_selection(PaletteSelectionMove::Next);
    assert!(empty.selected(&palette).is_none());
    Ok(())
}
