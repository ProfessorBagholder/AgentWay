-- tusd is authoritative for offsets; no independently writable byte ledger.
ALTER TABLE media_uploads DROP COLUMN offset;
ALTER TABLE media_uploads ADD COLUMN transport_id TEXT;
CREATE TABLE media_transfer_attempts (
 id TEXT PRIMARY KEY,
 media_id TEXT NOT NULL REFERENCES media_uploads(media_id),
 created_at INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE INDEX media_transfer_attempt_media ON media_transfer_attempts(media_id);
