-- Offsets here are observations for the owner UI. tusd remains authoritative for resume.
ALTER TABLE media_uploads ADD COLUMN observed_offset INTEGER NOT NULL DEFAULT 0;
ALTER TABLE media_uploads ADD COLUMN agent_name TEXT;
ALTER TABLE media_uploads ADD COLUMN created_at TEXT;
ALTER TABLE media_uploads ADD COLUMN last_error TEXT;
UPDATE media_uploads SET agent_name=(SELECT name FROM agent_connections WHERE id=media_uploads.agent_id);
UPDATE media_uploads SET created_at=(SELECT strftime('%Y-%m-%dT%H:%M:%fZ', created_at, 'unixepoch') FROM media WHERE id=media_uploads.media_id);
CREATE INDEX media_uploads_created ON media_uploads(created_at DESC, media_id DESC);
