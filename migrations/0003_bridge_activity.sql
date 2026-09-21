CREATE TABLE bridge_activity (
    id INTEGER PRIMARY KEY CHECK(id=1),
    last_seen TEXT NOT NULL,
    operation TEXT NOT NULL,
    revision INTEGER NOT NULL DEFAULT 1
);
