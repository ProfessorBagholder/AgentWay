CREATE TABLE publishing_settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE oauth_attempts (state TEXT PRIMARY KEY, verifier TEXT NOT NULL, redirect_uri TEXT NOT NULL, expires_at INTEGER NOT NULL);
CREATE TABLE youtube_account (id INTEGER PRIMARY KEY CHECK(id=1), channel_id TEXT NOT NULL, channel_name TEXT NOT NULL, refresh_token TEXT NOT NULL);
CREATE TABLE media (id TEXT PRIMARY KEY, size INTEGER NOT NULL, mime TEXT NOT NULL, ready INTEGER NOT NULL DEFAULT 0, created_at INTEGER NOT NULL DEFAULT (unixepoch()));
CREATE TABLE publications (
 id TEXT PRIMARY KEY, request_id TEXT NOT NULL UNIQUE, input TEXT NOT NULL,
 channel_id TEXT NOT NULL, media_id TEXT NOT NULL REFERENCES media(id),
 title TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'queued',
 uploaded_bytes INTEGER NOT NULL DEFAULT 0, total_bytes INTEGER NOT NULL,
 session TEXT, video_id TEXT, video_url TEXT, error TEXT,
 created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
 revision INTEGER NOT NULL DEFAULT 1
);
