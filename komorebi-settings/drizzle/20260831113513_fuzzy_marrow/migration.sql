CREATE TABLE `file_search_settings` (
	`singleton` INTEGER PRIMARY KEY,
	`root_wtf16` BLOB NOT NULL,
	CONSTRAINT `file_search_settings_singleton_check` CHECK(singleton = 1)
) STRICT;