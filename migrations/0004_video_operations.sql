CREATE TABLE video_operations (
 request_id TEXT PRIMARY KEY,
 publication_id TEXT NOT NULL REFERENCES publications(id),
 privacy TEXT NOT NULL,
 replacement_id TEXT REFERENCES publications(id),
 status TEXT NOT NULL DEFAULT 'pending',
 result TEXT,
 error TEXT,
 created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE INDEX video_operations_publication ON video_operations(publication_id);
