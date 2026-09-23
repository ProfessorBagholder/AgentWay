CREATE TABLE podcast_operations (
    request_id TEXT PRIMARY KEY,
    channel_id TEXT NOT NULL,
    input TEXT NOT NULL,
    result TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
