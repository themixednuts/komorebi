use std::num::NonZeroU16;

use komorebi_protocol::ActionAvailability;
use komorebi_protocol::ActionCategory;
use komorebi_protocol::ActionDefinition;
use komorebi_protocol::ActionDefinitionSpec;
use komorebi_protocol::ActionId;
use komorebi_protocol::ActionKey;
use komorebi_protocol::ActionOffer;
use komorebi_protocol::ActionSchemaVersion;
use komorebi_protocol::BoundedText;
use komorebi_protocol::CatalogCodec;
use komorebi_protocol::CatalogSnapshot;
use komorebi_protocol::CatalogStamp;
use komorebi_protocol::ConfirmationPolicy;
use komorebi_protocol::ManagerEpoch;
use komorebi_protocol::OfferRef;
use komorebi_protocol::PermittedUse;
use komorebi_protocol::Revision;
use komorebi_protocol::StateStamp;
use komorebi_protocol::UndoPolicy;
use komorebi_shell::ShortcutGuide;

fn catalog() -> Result<CatalogSnapshot, Box<dyn std::error::Error>> {
    let epoch = ManagerEpoch::new([73; 16])?;
    let revision = Revision::FIRST;
    let stamp = CatalogStamp::new(epoch, revision, revision, revision);
    let state = StateStamp::new(epoch, revision);
    let key = ActionKey::new(
        ActionId::parse("focus-window")?,
        ActionSchemaVersion::new(NonZeroU16::MIN),
    );
    let definition = ActionDefinition::new(ActionDefinitionSpec {
        key: key.clone(),
        category: ActionCategory::Window,
        title: BoundedText::new("Focus window")?,
        description: BoundedText::new("Focus the neighboring window")?,
        keywords: vec![BoundedText::new("navigation")?],
        parameters: vec![],
        permitted_uses: vec![PermittedUse::Interactive],
        confirmation: ConfirmationPolicy::None,
        undo: UndoPolicy::None,
    })?;
    let fingerprint = CatalogCodec::definition_fingerprint(&definition)?;
    let offer = ActionOffer::new(
        OfferRef::new(key, fingerprint, stamp),
        state,
        ActionAvailability::Available,
        None,
        vec![],
        vec![BoundedText::new("alt+h")?, BoundedText::new("alt+left")?],
    )?;
    Ok(CatalogSnapshot::new(
        stamp,
        state,
        vec![definition],
        vec![offer],
    )?)
}

#[test]
fn guide_projects_authoritative_bindings_and_searches_action_metadata()
-> Result<(), Box<dyn std::error::Error>> {
    let guide = ShortcutGuide::project(&catalog()?);

    assert_eq!(guide.entries().len(), 2);
    assert_eq!(guide.entries()[0].trigger(), "alt+h");
    assert_eq!(guide.entries()[0].action_id(), "focus-window");
    assert_eq!(guide.entries()[0].title(), "Focus window");
    assert_eq!(
        guide.entries()[0].description(),
        "Focus the neighboring window"
    );
    assert_eq!(guide.entries()[0].category(), ActionCategory::Window);
    assert_eq!(
        guide.entries()[0].availability(),
        ActionAvailability::Available
    );
    assert_eq!(
        guide
            .search("NAVIGATION")
            .map(komorebi_shell::ShortcutGuideEntry::trigger)
            .collect::<Vec<_>>(),
        vec!["alt+h", "alt+left"]
    );
    assert_eq!(
        guide
            .search("neighboring")
            .map(komorebi_shell::ShortcutGuideEntry::trigger)
            .collect::<Vec<_>>(),
        vec!["alt+h", "alt+left"]
    );
    assert_eq!(guide.search("resize").count(), 0);
    Ok(())
}
