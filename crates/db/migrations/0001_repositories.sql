-- The repositories added to Gitto, in the order their tabs are in.
CREATE TABLE `repositories` (
	`id` text PRIMARY KEY,
	`name` text NOT NULL,
	`path` text NOT NULL UNIQUE,
	`position` integer DEFAULT 0 NOT NULL
);
