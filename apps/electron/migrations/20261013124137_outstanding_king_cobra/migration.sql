ALTER TABLE `repositories` ADD `position` integer DEFAULT 0 NOT NULL;--> statement-breakpoint
-- The tabs keep the order they had, the order the repositories were added in.
UPDATE `repositories` SET `position` = (SELECT count(*) FROM `repositories` AS `earlier` WHERE `earlier`.`rowid` < `repositories`.`rowid`);
