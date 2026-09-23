CREATE TABLE youtube_asset_operations (
 request_id TEXT PRIMARY KEY,
 publication_id TEXT NOT NULL REFERENCES publications(id),
 agent_id TEXT NOT NULL REFERENCES agent_connections(id),
 input TEXT NOT NULL,
 result TEXT NOT NULL,
 created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
