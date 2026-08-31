use drizzle::sqlite::prelude::*;

#[SQLiteTable(NAME = "web_search_settings", STRICT)]
pub(crate) struct WebSearchSettings {
    #[column(primary, check = "singleton = 1")]
    pub singleton: i64,
    pub base_url: String,
    pub query_parameter: String,
}

#[derive(Debug, SQLiteFromRow)]
#[from(WebSearchSettings)]
pub(crate) struct WebSearchRow {
    pub base_url: String,
    pub query_parameter: String,
}

#[SQLiteTable(NAME = "file_search_settings", STRICT)]
pub(crate) struct FileSearchSettings {
    #[column(primary, check = "singleton = 1")]
    pub singleton: i64,
    #[column(blob)]
    pub root_wtf16: Vec<u8>,
}

#[derive(Debug, SQLiteFromRow)]
#[from(FileSearchSettings)]
pub(crate) struct FileSearchRow {
    pub root_wtf16: Vec<u8>,
}

// Drizzle generates an explicit `Clone` implementation for this `Copy` schema
// handle outside this crate's control.
#[allow(clippy::expl_impl_clone_on_copy)]
#[derive(SQLiteSchema)]
pub(crate) struct SettingsSchema {
    pub web_search: WebSearchSettings,
    pub file_search: FileSearchSettings,
}
