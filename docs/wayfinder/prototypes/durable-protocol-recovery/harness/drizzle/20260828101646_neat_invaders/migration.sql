CREATE TABLE `InvocationLedger` (
	`identity` TEXT PRIMARY KEY NOT NULL,
	`principal` TEXT NOT NULL,
	`invocation_id` INTEGER NOT NULL,
	`digest` BLOB NOT NULL,
	`phase` TEXT NOT NULL,
	`manager_revision` INTEGER NOT NULL,
	`effect_kind` TEXT NOT NULL,
	`outcome` TEXT,
	`parameters` BLOB NOT NULL
);
--> statement-breakpoint
CREATE TABLE `PrincipalFloors` (
	`principal` TEXT PRIMARY KEY NOT NULL,
	`minimum_accepted` INTEGER NOT NULL
);
--> statement-breakpoint
CREATE TABLE `CommittedEvents` (
	`position` INTEGER PRIMARY KEY,
	`manager_epoch` BLOB NOT NULL,
	`manager_revision` INTEGER NOT NULL,
	`invocation_identity` TEXT NOT NULL,
	`topic` TEXT NOT NULL
);