ALTER TABLE video_operations ADD COLUMN action TEXT NOT NULL DEFAULT 'visibility';
ALTER TABLE video_operations ADD COLUMN provider_attempted INTEGER NOT NULL DEFAULT 0;
ALTER TABLE publications ADD COLUMN deleted_at TEXT;
