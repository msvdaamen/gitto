-- The repositories added to Gitto, in the order their tabs are in.
--
-- `IF NOT EXISTS`, as a database the Electron app made already has this table: the app opens it
-- with the repositories in it, then migrates it on from here.
CREATE TABLE IF NOT EXISTS `repositories` (
	`id` text PRIMARY KEY,
	`name` text NOT NULL,
	`path` text NOT NULL UNIQUE,
	`position` integer DEFAULT 0 NOT NULL
);
