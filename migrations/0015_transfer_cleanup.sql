ALTER TABLE media_transfer_attempts ADD COLUMN last_cleanup INTEGER NOT NULL DEFAULT 0;
CREATE INDEX media_transfer_cleanup ON media_transfer_attempts(last_cleanup);
