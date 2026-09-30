CREATE TABLE `repositories` (
	`id` text PRIMARY KEY,
	`name` text NOT NULL,
	`path` text NOT NULL UNIQUE
);
