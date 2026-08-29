use drizzle::sqlite::prelude::*;

#[SQLiteTable(name = "ToolchainFacts")]
pub struct ToolchainFactRow {
    #[column(primary)]
    pub id: i64,
    pub label: String,
    pub native_payload: Vec<u8>,
}

#[allow(clippy::expl_impl_clone_on_copy)]
#[derive(SQLiteSchema)]
pub struct CompatibilitySchema {
    pub facts: ToolchainFactRow,
}
