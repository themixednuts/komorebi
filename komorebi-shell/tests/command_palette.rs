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
use komorebi_search::ContentSearchLimit;
use komorebi_search::FileSearchLimit;
use komorebi_search::FileSearchQueueCapacity;
use komorebi_search::FileSearchService;
use komorebi_shell::CommandPalette;
use komorebi_shell::FileActivationQueueCapacity;
use komorebi_shell::FileActivationService;
use komorebi_shell::FileLaunchFailure;
use komorebi_shell::FileLauncher;
use komorebi_shell::PaletteActionState;
use komorebi_shell::PaletteCompletion;
use komorebi_shell::PaletteCompletionDisposition;
use komorebi_shell::PaletteContent;
use komorebi_shell::PaletteController;
use komorebi_shell::PaletteEffect;
use komorebi_shell::PaletteMatches;
use komorebi_shell::PaletteQuery;
use komorebi_shell::PaletteResults;
use komorebi_shell::PaletteSearchBroker;
use komorebi_shell::PaletteSearchCompletionDisposition;
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
        PaletteResults::ContentPrompt
        | PaletteResults::ContentSearch(_)
        | PaletteResults::WebPrompt
        | PaletteResults::WebSearch(_) => Err("query should produce local action results"),
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
            .action_at(&palette, 0)
            .ok_or("focus search should match")?
            .action_id(),
        "focus-window"
    );
    assert_eq!(
        action_results(&palette, "neigbor")?
            .action_at(&palette, 0)
            .ok_or("neighbor search should match")?
            .title(),
        "Focus window"
    );

    let focus_results = action_results(&palette, "focus")?;
    let focus = focus_results
        .action_at(&palette, 0)
        .ok_or("focus search should match")?;
    let PaletteActionState::Ready(binding) = focus.state() else {
        return Err("focus-window should be immediately invokable".into());
    };
    assert_eq!(binding.action().as_str(), "focus-window");

    let close_results = action_results(&palette, "close")?;
    let close = close_results
        .action_at(&palette, 0)
        .ok_or("close search should match")?;
    assert_eq!(
        close.state(),
        PaletteActionState::Unavailable(ActionUnavailability::NoFocusedWindow)
    );

    let move_results = action_results(&palette, "move")?;
    let move_window = move_results
        .action_at(&palette, 0)
        .ok_or("move search should match")?;
    let PaletteActionState::RequiresInput(parameters) = move_window.state() else {
        return Err("move-window should require its direction".into());
    };
    assert_eq!(parameters.len(), 1);
    assert_eq!(parameters[0].id().as_str(), "direction");
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

    assert_eq!(PaletteQuery::parse("  ?  "), PaletteQuery::ContentPrompt);
    let PaletteQuery::ContentSearch(search) = PaletteQuery::parse(" ?  native compositor ") else {
        return Err("a non-empty question query should search indexed file contents".into());
    };
    assert_eq!(search.as_str(), "native compositor");
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
            .action_at(&palette, 0)
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
    let PaletteResults::ContentSearch(terms) =
        palette.query(PaletteQuery::parse("? compositor ownership"))
    else {
        return Err("content terms should retain indexed-search authority".into());
    };
    assert_eq!(terms.as_str(), "compositor ownership");
    Ok(())
}

#[test]
fn palette_controller_owns_query_selection_and_single_action_activation()
-> Result<(), Box<dyn std::error::Error>> {
    let mut controller = PaletteController::new(CommandPalette::project(&catalog()?));

    _ = controller.update_query("focus");
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
    _ = controller.update_query("focus");

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

    _ = controller.update_query("this cannot match any action");
    assert!(controller.is_empty());
    assert_eq!(controller.actions().count(), 0);
    assert_eq!(controller.selected_position(), None);
    Ok(())
}

#[test]
fn palette_controller_emits_one_typed_web_activation_for_bang_terms()
-> Result<(), Box<dyn std::error::Error>> {
    let mut controller = PaletteController::new(CommandPalette::project(&catalog()?));

    _ = controller.update_query("! rust windows shell");
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
    _ = controller.update_query("! typed rust");
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
    _ = controller.update_query("! no implicit provider");
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

#[tokio::test]
async fn palette_applies_file_results_from_its_typed_query_effect()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    std::fs::write(
        directory.path().join("command-palette.rs"),
        b"fn palette() {}",
    )?;
    let files = FileSearchService::start(
        directory.path().to_path_buf(),
        FileSearchQueueCapacity::new(1).ok_or("one is a valid capacity")?,
    )
    .await?;
    let broker = PaletteSearchBroker::configured(
        files.client(),
        FileSearchLimit::new(8).ok_or("eight is a valid limit")?,
        ContentSearchLimit::new(8).ok_or("eight is a valid content limit")?,
    );
    let mut controller = PaletteController::new(CommandPalette::project(&catalog()?));

    let search = controller
        .update_query("command palete")
        .ok_or("nonempty local terms should request file search")?;
    let completion = search.submit(&broker).await;
    assert_eq!(
        controller.complete_search(completion),
        PaletteSearchCompletionDisposition::Applied
    );
    assert_eq!(
        controller
            .files()
            .map(komorebi_search::FileSearchMatch::display_path)
            .collect::<Vec<_>>(),
        ["command-palette.rs"]
    );
    assert_eq!(controller.selected_position(), Some(0));
    assert_eq!(
        controller
            .selected_file()
            .ok_or("the first file row should become selected")?
            .display_path(),
        "command-palette.rs"
    );

    files.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn palette_ignores_results_from_a_superseded_provider_query()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    std::fs::write(directory.path().join("alpha.rs"), b"alpha")?;
    std::fs::write(directory.path().join("beta.rs"), b"beta")?;
    let files = FileSearchService::start(
        directory.path().to_path_buf(),
        FileSearchQueueCapacity::new(2).ok_or("two is a valid capacity")?,
    )
    .await?;
    let broker = PaletteSearchBroker::configured(
        files.client(),
        FileSearchLimit::new(8).ok_or("eight is a valid limit")?,
        ContentSearchLimit::new(8).ok_or("eight is a valid content limit")?,
    );
    let mut controller = PaletteController::new(CommandPalette::project(&catalog()?));

    let alpha = controller
        .update_query("alpha")
        .ok_or("first local query should request file search")?;
    let beta = controller
        .update_query("? beta")
        .ok_or("second query should request content search")?;
    assert_eq!(
        controller.complete_search(alpha.submit(&broker).await),
        PaletteSearchCompletionDisposition::IgnoredStale
    );
    assert_eq!(controller.files().count(), 0);
    assert_eq!(
        controller.complete_search(beta.submit(&broker).await),
        PaletteSearchCompletionDisposition::Applied
    );
    assert_eq!(
        controller
            .content_results()
            .map(komorebi_search::ContentSearchMatch::display_path)
            .collect::<Vec<_>>(),
        ["beta.rs"]
    );

    files.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn palette_applies_indexed_content_results_from_an_explicit_content_query()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let exact_path = directory.path().join("renderer-notes.md");
    std::fs::write(
        &exact_path,
        b"first line\nnative compositor owns the visual\nlast line",
    )?;
    let files = FileSearchService::start(
        directory.path().to_path_buf(),
        FileSearchQueueCapacity::new(1).ok_or("one is a valid capacity")?,
    )
    .await?;
    let broker = PaletteSearchBroker::configured(
        files.client(),
        FileSearchLimit::new(8).ok_or("eight is a valid file limit")?,
        ContentSearchLimit::new(8).ok_or("eight is a valid content limit")?,
    );
    let launcher = RecordingFileLauncher::default();
    let activation = FileActivationService::start(
        files.client(),
        launcher.clone(),
        FileActivationQueueCapacity::new(1).ok_or("one is a valid activation capacity")?,
    );
    let mut controller = PaletteController::new(CommandPalette::project(&catalog()?));

    let search = controller
        .update_query("? native compositor")
        .ok_or("explicit content terms should request indexed search")?;
    assert_eq!(
        controller.complete_search(search.submit(&broker).await),
        PaletteSearchCompletionDisposition::Applied
    );
    let result = controller
        .content_results()
        .next()
        .ok_or("matching indexed content should be visible")?;
    assert_eq!(result.display_path(), "renderer-notes.md");
    assert_eq!(result.line_number().get(), 2);
    assert_eq!(result.line_content(), "native compositor owns the visual");
    assert_eq!(controller.selected_position(), Some(0));
    let Some(PaletteEffect::File(invocation)) = controller.activate() else {
        return Err("selected content should emit exact-file activation".into());
    };
    let submission = invocation.submit(&activation.client()).await;
    assert_eq!(
        controller.complete(submission.complete().await),
        PaletteCompletionDisposition::Applied
    );
    assert_eq!(
        launcher
            .paths
            .lock()
            .map_err(|_| "recording launcher lock was poisoned")?
            .as_slice(),
        [exact_path]
    );

    activation.shutdown().await?;
    files.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn one_controller_cursor_moves_across_action_and_file_rows()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    std::fs::write(directory.path().join("focus-window-notes.md"), b"focus")?;
    let files = FileSearchService::start(
        directory.path().to_path_buf(),
        FileSearchQueueCapacity::new(1).ok_or("one is a valid capacity")?,
    )
    .await?;
    let broker = PaletteSearchBroker::configured(
        files.client(),
        FileSearchLimit::new(8).ok_or("eight is a valid limit")?,
        ContentSearchLimit::new(8).ok_or("eight is a valid content limit")?,
    );
    let mut controller = PaletteController::new(CommandPalette::project(&catalog()?));

    let search = controller
        .update_query("focus window")
        .ok_or("local terms should request file search")?;
    assert_eq!(
        controller.complete_search(search.submit(&broker).await),
        PaletteSearchCompletionDisposition::Applied
    );
    let action_count = controller.actions().count();
    assert!(action_count > 0);
    assert_eq!(controller.selected_position(), Some(0));
    assert!(controller.selected_action().is_some());
    assert!(controller.selected_file().is_none());

    for _ in 0..action_count {
        controller.move_selection(PaletteSelectionMove::Next);
    }
    assert_eq!(controller.selected_position(), Some(action_count));
    assert!(controller.selected_action().is_none());
    assert_eq!(
        controller
            .selected_file()
            .ok_or("second row should be the file result")?
            .display_path(),
        "focus-window-notes.md"
    );

    controller.move_selection(PaletteSelectionMove::Next);
    assert_eq!(controller.selected_position(), Some(0));
    assert!(controller.selected_action().is_some());
    files.shutdown().await?;
    Ok(())
}

#[derive(Clone, Default)]
struct RecordingFileLauncher {
    paths: std::sync::Arc<std::sync::Mutex<Vec<std::path::PathBuf>>>,
}

impl FileLauncher for RecordingFileLauncher {
    async fn launch(&self, path: std::path::PathBuf) -> Result<(), FileLaunchFailure> {
        self.paths
            .lock()
            .map_err(|_| FileLaunchFailure::new("recording launcher lock was poisoned"))?
            .push(path);
        Ok(())
    }
}

#[tokio::test]
async fn selected_file_activation_resolves_and_launches_the_exact_index_path()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let exact_path = directory.path().join("launch-me.txt");
    std::fs::write(&exact_path, b"launch")?;
    let files = FileSearchService::start(
        directory.path().to_path_buf(),
        FileSearchQueueCapacity::new(2).ok_or("two is a valid search capacity")?,
    )
    .await?;
    let search = PaletteSearchBroker::configured(
        files.client(),
        FileSearchLimit::new(8).ok_or("eight is a valid limit")?,
        ContentSearchLimit::new(8).ok_or("eight is a valid content limit")?,
    );
    let launcher = RecordingFileLauncher::default();
    let activation = FileActivationService::start(
        files.client(),
        launcher.clone(),
        FileActivationQueueCapacity::new(1).ok_or("one is a valid activation capacity")?,
    );
    let mut controller = PaletteController::new(CommandPalette::project(&catalog()?));

    let query = controller
        .update_query("launch me")
        .ok_or("local terms should request file search")?;
    assert_eq!(
        controller.complete_search(query.submit(&search).await),
        PaletteSearchCompletionDisposition::Applied
    );
    let Some(PaletteEffect::File(invocation)) = controller.activate() else {
        return Err("selected file should emit brokered activation".into());
    };
    let submission = invocation.submit(&activation.client()).await;
    assert_eq!(
        controller.complete(submission.complete().await),
        PaletteCompletionDisposition::Applied
    );
    assert!(matches!(
        controller.status(),
        PaletteStatus::Succeeded { .. }
    ));
    assert_eq!(
        launcher
            .paths
            .lock()
            .map_err(|_| "recording launcher lock was poisoned")?
            .as_slice(),
        [exact_path]
    );

    activation.shutdown().await?;
    files.shutdown().await?;
    Ok(())
}
