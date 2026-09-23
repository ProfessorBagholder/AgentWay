-- Session tombstones preserve creation idempotency after cancellation/expiry.
CREATE TABLE media_uploads (
 media_id TEXT PRIMARY KEY,
 agent_id TEXT NOT NULL,
 request_id TEXT NOT NULL,
 size INTEGER NOT NULL,
 mime TEXT NOT NULL,
 sha256 TEXT NOT NULL,
 offset INTEGER NOT NULL DEFAULT 0,
 status TEXT NOT NULL DEFAULT 'receiving',
 expires_at INTEGER NOT NULL DEFAULT (unixepoch()+604800),
 UNIQUE(agent_id,request_id)
);
CREATE INDEX media_uploads_expiration ON media_uploads(status,expires_at);
