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
use komorebi_shell::PaletteCompletion;
use komorebi_shell::PaletteCompletionDisposition;
use komorebi_shell::PaletteContent;
use komorebi_shell::PaletteController;
use komorebi_shell::PaletteEffect;
use komorebi_shell::PaletteMatches;
use komorebi_shell::PaletteQuery;
use komorebi_shell::PaletteResults;
use komorebi_shell::PaletteSelectionMove;
use komorebi_shell::PaletteStatus;
use komorebi_shell::PaletteSubmission;
use komorebi_shell::WebActivationQueueCapacity;
use komorebi_shell::WebActivationService;
use komorebi_shell::WebLaunchDisposition;
use komorebi_shell::WebLaunchFailure;
use komorebi_shell::WebSearchBroker;
use komorebi_shell::WebSearchEndpoint;
use komorebi_shell::WebSearchTarget;
use komorebi_shell::WebUriLauncher;

#[derive(Clone, Copy)]
struct SuccessfulWebLauncher;

impl WebUriLauncher for SuccessfulWebLauncher {
    async fn launch(&self, _: WebSearchTarget) -> Result<WebLaunchDisposition, WebLaunchFailure> {
        Ok(WebLaunchDisposition::Launched)
    }
}

fn action_results(palette: &CommandPalette, input: &str) -> Result<PaletteMatches, &'static str> {
    match palette.query(PaletteQuery::parse(input)) {
        PaletteResults::Actions(matches) => Ok(matches),
        PaletteResults::WebPrompt | PaletteResults::WebSearch(_) => {
            Err("query should produce local action results")
        }
    }
}

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
        action_results(&palette, "foc windw")?
            .selected(&palette)
            .ok_or("focus search should match")?
            .action_id(),
        "focus-window"
    );
    assert_eq!(
        action_results(&palette, "neigbor")?
            .selected(&palette)
            .ok_or("neighbor search should match")?
            .title(),
        "Focus window"
    );

    let focus_results = action_results(&palette, "focus")?;
    let focus = focus_results
        .selected(&palette)
        .ok_or("focus search should match")?;
    let PaletteActionState::Ready(binding) = focus.state() else {
        return Err("focus-window should be immediately invokable".into());
    };
    assert_eq!(binding.action().as_str(), "focus-window");

    let close_results = action_results(&palette, "close")?;
    let close = close_results
        .selected(&palette)
        .ok_or("close search should match")?;
    assert_eq!(
        close.state(),
        PaletteActionState::Unavailable(ActionUnavailability::NoFocusedWindow)
    );

    let move_results = action_results(&palette, "move")?;
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
    let mut results = action_results(&palette, "")?;

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

    let mut empty = action_results(&palette, "this cannot match any catalog action")?;
    assert!(empty.selected(&palette).is_none());
    empty.move_selection(PaletteSelectionMove::Next);
    assert!(empty.selected(&palette).is_none());
    Ok(())
}

#[test]
fn palette_query_parser_makes_web_activation_explicit_and_non_empty()
-> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(PaletteQuery::parse("   "), PaletteQuery::Browse);

    let PaletteQuery::Search(search) = PaletteQuery::parse("  focus window  ") else {
        return Err("ordinary text should search first-party providers".into());
    };
    assert_eq!(search.as_str(), "focus window");

    assert_eq!(PaletteQuery::parse("  !  "), PaletteQuery::WebPrompt);
    let PaletteQuery::WebSearch(search) = PaletteQuery::parse(" !  rust wtf-16 paths ") else {
        return Err("a non-empty bang query should be a web search".into());
    };
    assert_eq!(search.as_str(), "rust wtf-16 paths");
    Ok(())
}

#[test]
fn palette_query_results_preserve_source_specific_activation_data()
-> Result<(), Box<dyn std::error::Error>> {
    let palette = CommandPalette::project(&catalog()?);

    let PaletteResults::Actions(actions) = palette.query(PaletteQuery::parse("focus")) else {
        return Err("local search should produce action matches".into());
    };
    assert_eq!(
        actions
            .selected(&palette)
            .ok_or("focus search should select an action")?
            .action_id(),
        "focus-window"
    );
    assert_eq!(
        palette.query(PaletteQuery::parse("!")),
        PaletteResults::WebPrompt
    );
    let PaletteResults::WebSearch(request) =
        palette.query(PaletteQuery::parse("! windows reactor"))
    else {
        return Err("web terms should produce a broker request".into());
    };
    assert_eq!(request.terms(), "windows reactor");
    Ok(())
}

#[test]
fn palette_controller_owns_query_selection_and_single_action_activation()
-> Result<(), Box<dyn std::error::Error>> {
    let mut controller = PaletteController::new(CommandPalette::project(&catalog()?));

    controller.update_query("focus");
    assert_eq!(
        controller
            .selected_action()
            .ok_or("focus query should select an action")?
            .action_id(),
        "focus-window"
    );

    let Some(PaletteEffect::Invoke(invocation)) = controller.activate() else {
        return Err("ready action should emit an invocation".into());
    };
    assert_eq!(invocation.binding().action().as_str(), "focus-window");
    assert!(matches!(
        controller.status(),
        PaletteStatus::Submitting { attempt, label }
            if *attempt == invocation.attempt() && label.as_ref() == "Focus window"
    ));
    assert!(controller.activate().is_none());
    Ok(())
}

#[test]
fn palette_controller_rejects_stale_completion_without_overwriting_current_attempt()
-> Result<(), Box<dyn std::error::Error>> {
    let mut controller = PaletteController::new(CommandPalette::project(&catalog()?));
    controller.update_query("focus");

    let Some(PaletteEffect::Invoke(first)) = controller.activate() else {
        return Err("first activation should emit an invocation".into());
    };
    assert_eq!(
        controller.complete(PaletteCompletion::succeeded(first.attempt())),
        PaletteCompletionDisposition::Applied
    );
    assert!(matches!(
        controller.status(),
        PaletteStatus::Succeeded { label } if label.as_ref() == "Focus window"
    ));

    let Some(PaletteEffect::Invoke(second)) = controller.activate() else {
        return Err("completed activation should permit another invocation".into());
    };
    assert_ne!(first.attempt(), second.attempt());
    assert_eq!(
        controller.complete(PaletteCompletion::succeeded(first.attempt())),
        PaletteCompletionDisposition::IgnoredStale
    );
    assert!(matches!(
        controller.status(),
        PaletteStatus::Submitting { attempt, .. } if *attempt == second.attempt()
    ));
    assert_eq!(
        controller.complete(PaletteCompletion::succeeded(second.attempt())),
        PaletteCompletionDisposition::Applied
    );
    Ok(())
}

#[test]
fn palette_controller_exposes_bounded_rows_and_selection_for_renderers()
-> Result<(), Box<dyn std::error::Error>> {
    let mut controller = PaletteController::new(CommandPalette::project(&catalog()?));

    assert_eq!(controller.actions().count(), 3);
    assert_eq!(controller.selected_position(), Some(0));
    controller.move_selection(PaletteSelectionMove::Previous);
    assert_eq!(controller.selected_position(), Some(2));
    assert!(!controller.select_position(3));
    assert!(controller.select_position(1));
    assert_eq!(
        controller
            .selected_action()
            .ok_or("selected row should resolve")?
            .action_id(),
        "focus-window"
    );

    controller.update_query("this cannot match any action");
    assert!(controller.is_empty());
    assert_eq!(controller.actions().count(), 0);
    assert_eq!(controller.selected_position(), None);
    Ok(())
}

#[test]
fn palette_controller_emits_one_typed_web_activation_for_bang_terms()
-> Result<(), Box<dyn std::error::Error>> {
    let mut controller = PaletteController::new(CommandPalette::project(&catalog()?));

    controller.update_query("! rust windows shell");
    assert!(matches!(
        controller.content(),
        PaletteContent::WebSearch(request) if request.terms() == "rust windows shell"
    ));

    let Some(PaletteEffect::Web(invocation)) = controller.activate() else {
        return Err("web terms should emit a brokered activation".into());
    };
    assert_eq!(invocation.request().terms(), "rust windows shell");
    assert!(matches!(
        controller.status(),
        PaletteStatus::Submitting { attempt, label }
            if *attempt == invocation.attempt() && label.as_ref() == "Search the web"
    ));
    assert!(controller.activate().is_none());
    Ok(())
}

#[tokio::test]
async fn palette_web_effect_completes_through_the_owned_broker()
-> Result<(), Box<dyn std::error::Error>> {
    let web = WebActivationService::start(
        WebSearchEndpoint::new("https://search.example/results", "q")?,
        SuccessfulWebLauncher,
        WebActivationQueueCapacity::new(1).ok_or("one is a valid capacity")?,
    );
    let mut controller = PaletteController::new(CommandPalette::project(&catalog()?));
    controller.update_query("! typed rust");
    let Some(PaletteEffect::Web(invocation)) = controller.activate() else {
        return Err("web terms should emit a brokered activation".into());
    };

    let broker = WebSearchBroker::Configured(web.client());
    let submission = invocation.submit(&broker).await;
    assert!(matches!(submission, PaletteSubmission::Pending(_)));
    assert_eq!(
        controller.complete(submission.complete().await),
        PaletteCompletionDisposition::Applied
    );
    assert!(matches!(
        controller.status(),
        PaletteStatus::Succeeded { label } if label.as_ref() == "Search the web"
    ));
    web.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn unconfigured_web_search_completes_with_a_typed_failure()
-> Result<(), Box<dyn std::error::Error>> {
    let mut controller = PaletteController::new(CommandPalette::project(&catalog()?));
    controller.update_query("! no implicit provider");
    let Some(PaletteEffect::Web(invocation)) = controller.activate() else {
        return Err("web terms should emit a brokered activation".into());
    };

    let submission = invocation.submit(&WebSearchBroker::Unconfigured).await;
    assert!(matches!(submission, PaletteSubmission::Complete(_)));
    assert_eq!(
        controller.complete(submission.complete().await),
        PaletteCompletionDisposition::Applied
    );
    assert!(matches!(
        controller.status(),
        PaletteStatus::Failed {
            label,
            failure: komorebi_shell::PaletteFailure::WebUnavailable,
        } if label.as_ref() == "Search the web"
    ));
    Ok(())
}
