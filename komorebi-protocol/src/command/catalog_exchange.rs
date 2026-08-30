use super::CatalogSnapshot;
use super::CatalogStamp;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CatalogQuery {
    known: Option<CatalogStamp>,
}

impl CatalogQuery {
    #[must_use]
    pub const fn new(known: Option<CatalogStamp>) -> Self {
        Self { known }
    }

    #[must_use]
    pub const fn known(self) -> Option<CatalogStamp> {
        self.known
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CatalogReply {
    NotModified(CatalogStamp),
    Snapshot(CatalogSnapshot),
}
