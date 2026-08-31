/// Exact Windows Shell parsing identity for one installed application.
///
/// The UTF-16 code units are intentionally opaque: display text is never a
/// launch operand, and ill-formed Windows strings remain lossless.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ApplicationId(Box<[u16]>);

impl ApplicationId {
    /// Creates an identity from a non-empty, non-NUL-terminated Shell parsing name.
    #[must_use]
    pub fn from_utf16(units: impl Into<Box<[u16]>>) -> Option<Self> {
        let units = units.into();
        (!units.is_empty() && !units.contains(&0)).then_some(Self(units))
    }

    pub(crate) fn nul_terminated(&self) -> Vec<u16> {
        let mut units = Vec::with_capacity(self.0.len() + 1);
        units.extend_from_slice(&self.0);
        units.push(0);
        units
    }
}

/// One installed application projected from the Windows Shell namespace.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationDescriptor {
    id: ApplicationId,
    name: Box<str>,
    search_text: Box<str>,
}

impl ApplicationDescriptor {
    #[must_use]
    pub fn new(id: ApplicationId, name: impl Into<Box<str>>) -> Self {
        let name = name.into();
        let search_text = name.to_lowercase().into_boxed_str();
        Self {
            id,
            name,
            search_text,
        }
    }

    #[must_use]
    pub const fn id(&self) -> &ApplicationId {
        &self.id
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn search_text(&self) -> &str {
        &self.search_text
    }
}

/// Immutable in-memory projection of launchable applications.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ApplicationCatalog {
    applications: Box<[ApplicationDescriptor]>,
}

impl ApplicationCatalog {
    #[must_use]
    pub fn new(mut applications: Vec<ApplicationDescriptor>) -> Self {
        applications.sort_unstable_by(|left, right| {
            left.name().to_lowercase().cmp(&right.name().to_lowercase())
        });
        let mut identities = HashSet::with_capacity(applications.len());
        applications.retain(|application| identities.insert(application.id.clone()));
        Self {
            applications: applications.into_boxed_slice(),
        }
    }

    #[must_use]
    pub const fn applications(&self) -> &[ApplicationDescriptor] {
        &self.applications
    }
}
use std::collections::HashSet;
