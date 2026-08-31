CREATE TABLE `web_search_settings` (
	`singleton` INTEGER PRIMARY KEY,
	`base_url` TEXT NOT NULL,
	`query_parameter` TEXT NOT NULL,
	CONSTRAINT `web_search_settings_singleton_check` CHECK(singleton = 1)
) STRICT;