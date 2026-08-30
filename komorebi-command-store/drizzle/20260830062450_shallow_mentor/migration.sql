CREATE TABLE `invocation_namespaces` (
	`namespace` BLOB PRIMARY KEY NOT NULL,
	`principal` BLOB NOT NULL,
	`next_sequence` BLOB NOT NULL,
	`minimum_accepted` BLOB NOT NULL,
	`record_count` INTEGER NOT NULL,
	CONSTRAINT `invocation_namespaces_record_count_check` CHECK(record_count >= 0 AND record_count <= 65536)
) STRICT;
--> statement-breakpoint
CREATE TABLE `invocation_records` (
	`namespace` BLOB NOT NULL,
	`sequence` BLOB NOT NULL,
	`principal` BLOB NOT NULL,
	`digest` BLOB NOT NULL,
	`parameters` BLOB NOT NULL,
	`phase` INTEGER NOT NULL,
	`recovery_policy` INTEGER,
	`logical_revision` BLOB,
	`terminal_kind` INTEGER,
	`outcome` BLOB,
	`committed_event` BLOB,
	`reserved_at_ms` INTEGER NOT NULL,
	`logical_committed_at_ms` INTEGER,
	`effect_dispatched_at_ms` INTEGER,
	`terminal_at_ms` INTEGER,
	CONSTRAINT `invocation_records_pk` PRIMARY KEY(`namespace`, `sequence`),
	CONSTRAINT `invocation_records_reserved_at_ms_check` CHECK(reserved_at_ms >= 0)
) STRICT;
--> statement-breakpoint
CREATE INDEX `invocation_recovery_idx` ON `invocation_records`(`phase`);
--> statement-breakpoint
CREATE INDEX `invocation_compaction_idx` ON `invocation_records`(`namespace`, `terminal_at_ms`);