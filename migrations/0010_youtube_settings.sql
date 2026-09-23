CREATE TABLE youtube_settings_operations (
 request_id TEXT PRIMARY KEY,
 publication_id TEXT NOT NULL REFERENCES publications(id),
 agent_id TEXT NOT NULL REFERENCES agent_connections(id),
 input TEXT NOT NULL,
 desired TEXT NOT NULL,
 status TEXT NOT NULL,
 error TEXT,
 created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE INDEX youtube_settings_publication ON youtube_settings_operations(publication_id);
