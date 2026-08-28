use drizzle::sqlite::prelude::*;

#[SQLiteTable(name = "PromotionJournal")]
pub struct PromotionJournalRow {
    #[column(primary)]
    pub sequence: i64,
    pub transaction: String,
    pub prior_installation: String,
    pub candidate_installation: String,
    pub fault_profile: String,
    pub boundary: String,
    pub previous_digest: String,
    pub digest: String,
}

#[SQLiteTable(name = "InternalConfiguration")]
pub struct InternalConfigurationRow {
    #[column(primary)]
    pub transaction: String,
    pub schema_version: i64,
    pub source_digest: String,
}

#[SQLiteTable(name = "ConfigurationWorkspaces")]
pub struct ConfigurationWorkspaceRow {
    #[column(primary, autoincrement)]
    pub id: i64,
    pub transaction: String,
    pub position: i64,
    pub name: String,
}

#[SQLiteTable(name = "ConfigurationBindings")]
pub struct ConfigurationBindingRow {
    #[column(primary, autoincrement)]
    pub id: i64,
    pub transaction: String,
    pub position: i64,
    pub workspace_position: i64,
}

#[SQLiteTable(name = "CandidateSeals")]
pub struct CandidateSealRow {
    #[column(primary)]
    pub transaction: String,
    pub installation: String,
    pub payload_digest: String,
    pub configuration_digest: String,
}

#[SQLiteTable(name = "WindowRecoverySnapshots")]
pub struct WindowRecoverySnapshotRow {
    #[column(primary)]
    pub transaction: String,
    pub focused_window: String,
    pub appearance_digest: String,
    pub reconciled: bool,
}

#[SQLiteTable(name = "WindowRecoveryPlacements")]
pub struct WindowRecoveryPlacementRow {
    #[column(primary, autoincrement)]
    pub id: i64,
    pub transaction: String,
    pub window_identity: String,
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
}

#[SQLiteTable(name = "NativePathFacts")]
pub struct NativePathFactRow {
    #[column(primary)]
    pub transaction: String,
    pub encoding_version: i64,
    pub code_units: Vec<u8>,
}

#[allow(clippy::expl_impl_clone_on_copy)]
#[derive(SQLiteSchema)]
pub struct PromotionSchema {
    pub journal: PromotionJournalRow,
    pub configurations: InternalConfigurationRow,
    pub configuration_workspaces: ConfigurationWorkspaceRow,
    pub configuration_bindings: ConfigurationBindingRow,
    pub candidate_seals: CandidateSealRow,
    pub window_snapshots: WindowRecoverySnapshotRow,
    pub window_placements: WindowRecoveryPlacementRow,
    pub native_paths: NativePathFactRow,
}
