CREATE TABLE `PromotionJournal` (
	`sequence` INTEGER PRIMARY KEY,
	`transaction` TEXT NOT NULL,
	`prior_installation` TEXT NOT NULL,
	`candidate_installation` TEXT NOT NULL,
	`fault_profile` TEXT NOT NULL,
	`boundary` TEXT NOT NULL,
	`previous_digest` TEXT NOT NULL,
	`digest` TEXT NOT NULL
);
--> statement-breakpoint
CREATE TABLE `InternalConfiguration` (
	`transaction` TEXT PRIMARY KEY NOT NULL,
	`schema_version` INTEGER NOT NULL,
	`source_digest` TEXT NOT NULL
);
--> statement-breakpoint
CREATE TABLE `ConfigurationWorkspaces` (
	`id` INTEGER PRIMARY KEY AUTOINCREMENT,
	`transaction` TEXT NOT NULL,
	`position` INTEGER NOT NULL,
	`name` TEXT NOT NULL
);
--> statement-breakpoint
CREATE TABLE `ConfigurationBindings` (
	`id` INTEGER PRIMARY KEY AUTOINCREMENT,
	`transaction` TEXT NOT NULL,
	`position` INTEGER NOT NULL,
	`workspace_position` INTEGER NOT NULL
);
--> statement-breakpoint
CREATE TABLE `CandidateSeals` (
	`transaction` TEXT PRIMARY KEY NOT NULL,
	`installation` TEXT NOT NULL,
	`payload_digest` TEXT NOT NULL,
	`configuration_digest` TEXT NOT NULL
);
--> statement-breakpoint
CREATE TABLE `WindowRecoverySnapshots` (
	`transaction` TEXT PRIMARY KEY NOT NULL,
	`focused_window` TEXT NOT NULL,
	`appearance_digest` TEXT NOT NULL,
	`reconciled` INTEGER NOT NULL
);
--> statement-breakpoint
CREATE TABLE `WindowRecoveryPlacements` (
	`id` INTEGER PRIMARY KEY AUTOINCREMENT,
	`transaction` TEXT NOT NULL,
	`window_identity` TEXT NOT NULL,
	`x` INTEGER NOT NULL,
	`y` INTEGER NOT NULL,
	`width` INTEGER NOT NULL,
	`height` INTEGER NOT NULL
);
--> statement-breakpoint
CREATE TABLE `NativePathFacts` (
	`transaction` TEXT PRIMARY KEY NOT NULL,
	`encoding_version` INTEGER NOT NULL,
	`code_units` BLOB NOT NULL
);