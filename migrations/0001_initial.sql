CREATE TABLE agents (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 80),
    platform TEXT NOT NULL CHECK(platform IN ('grok_bot','muse','chatgpt','claude')),
    role TEXT NOT NULL CHECK(role IN ('manager','worker')),
    status TEXT NOT NULL DEFAULT 'unconfigured'
);
CREATE TABLE tasks (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL CHECK(length(title) BETWEEN 1 AND 160),
    instructions TEXT NOT NULL,
    agent_id TEXT NOT NULL REFERENCES agents(id),
    status TEXT NOT NULL DEFAULT 'queued' CHECK(status IN ('queued','cancelled')),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    revision INTEGER NOT NULL DEFAULT 1,
    request_id TEXT NOT NULL UNIQUE
);
CREATE INDEX tasks_created ON tasks(created_at DESC, id DESC);
CREATE TABLE events (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL,
    payload TEXT NOT NULL
);
