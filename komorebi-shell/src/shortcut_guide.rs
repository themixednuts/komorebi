use komorebi_protocol::ActionAvailability;
use komorebi_protocol::ActionCategory;
use komorebi_protocol::ActionId;
use komorebi_protocol::CatalogSnapshot;

/// An immutable renderer-independent view of the manager's configured shortcuts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShortcutGuide {
    entries: Box<[ShortcutGuideEntry]>,
}

impl ShortcutGuide {
    /// Projects one owned row for every binding hint in an authorized catalog.
    #[must_use]
    pub fn project(catalog: &CatalogSnapshot) -> Self {
        let entries = catalog
            .definitions()
            .iter()
            .zip(catalog.offers())
            .flat_map(|(definition, offer)| {
                offer.bindings().iter().map(move |trigger| {
                    ShortcutGuideEntry::new(
                        trigger.as_str(),
                        definition.key().id().clone(),
                        definition.category(),
                        definition.title().as_str(),
                        definition.description().as_str(),
                        definition
                            .keywords()
                            .iter()
                            .map(komorebi_protocol::BoundedText::as_str),
                        offer.availability(),
                    )
                })
            })
            .collect();
        Self { entries }
    }

    #[must_use]
    pub fn entries(&self) -> &[ShortcutGuideEntry] {
        &self.entries
    }

    /// Returns rows containing the query in their trigger or action metadata.
    pub fn search<'a>(&'a self, query: &str) -> impl Iterator<Item = &'a ShortcutGuideEntry> + 'a {
        let query = query.trim().to_lowercase();
        self.entries
            .iter()
            .filter(move |entry| query.is_empty() || entry.search_text.contains(&query))
    }
}

/// One configured trigger paired with its authorized action presentation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShortcutGuideEntry {
    trigger: Box<str>,
    action_id: ActionId,
    category: ActionCategory,
    title: Box<str>,
    description: Box<str>,
    availability: ActionAvailability,
    search_text: Box<str>,
}

impl ShortcutGuideEntry {
    fn new<'a>(
        trigger: &str,
        action_id: ActionId,
        category: ActionCategory,
        title: &str,
        description: &str,
        keywords: impl Iterator<Item = &'a str>,
        availability: ActionAvailability,
    ) -> Self {
        let mut search_text = String::new();
        for value in [trigger, action_id.as_str(), title, description] {
            search_text.push_str(value);
            search_text.push('\n');
        }
        for keyword in keywords {
            search_text.push_str(keyword);
            search_text.push('\n');
        }
        let search_text = search_text.to_lowercase().into_boxed_str();
        Self {
            trigger: trigger.into(),
            action_id,
            category,
            title: title.into(),
            description: description.into(),
            availability,
            search_text,
        }
    }

    #[must_use]
    pub fn trigger(&self) -> &str {
        &self.trigger
    }

    #[must_use]
    pub fn action_id(&self) -> &str {
        self.action_id.as_str()
    }

    #[must_use]
    pub fn category(&self) -> ActionCategory {
        self.category
    }

    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }

    #[must_use]
    pub fn availability(&self) -> ActionAvailability {
        self.availability
    }
}
