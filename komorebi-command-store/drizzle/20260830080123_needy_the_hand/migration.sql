CREATE TABLE `invocation_leases` (
	`namespace_id` BLOB PRIMARY KEY NOT NULL,
	`principal` BLOB NOT NULL,
	`next_sequence` BLOB NOT NULL,
	`minimum_accepted` BLOB NOT NULL,
	`record_count` INTEGER NOT NULL,
	CONSTRAINT `invocation_leases_record_count_check` CHECK(record_count >= 0 AND record_count <= 65536)
) STRICT;
--> statement-breakpoint
CREATE TABLE `invocations` (
	`namespace` BLOB NOT NULL,
	`sequence` BLOB NOT NULL,
	`principal` BLOB NOT NULL,
	`digest` BLOB NOT NULL,
	`invocation` BLOB NOT NULL,
	`phase` INTEGER NOT NULL,
	`recovery_policy` INTEGER,
	`state_stamp` BLOB,
	`terminal_kind` INTEGER,
	`outcome` BLOB,
	`committed_event` BLOB,
	`reserved_at_ms` INTEGER NOT NULL,
	`logical_committed_at_ms` INTEGER,
	`effect_dispatched_at_ms` INTEGER,
	`terminal_at_ms` INTEGER,
	CONSTRAINT `invocations_pk` PRIMARY KEY(`namespace`, `sequence`),
	CONSTRAINT `invocations_state_stamp_check` CHECK(state_stamp IS NULL OR length(state_stamp) = 24),
	CONSTRAINT `invocations_reserved_at_ms_check` CHECK(reserved_at_ms >= 0)
) STRICT;
--> statement-breakpoint
DROP INDEX IF EXISTS `invocation_recovery_idx`;
--> statement-breakpoint
DROP INDEX IF EXISTS `invocation_compaction_idx`;
--> statement-breakpoint
CREATE INDEX `invocations_recovery_idx` ON `invocations`(`phase`);
--> statement-breakpoint
CREATE INDEX `invocations_compaction_idx` ON `invocations`(`namespace`, `terminal_at_ms`);
--> statement-breakpoint
DROP TABLE `invocation_namespaces`;
--> statement-breakpoint
DROP TABLE `invocation_records`;