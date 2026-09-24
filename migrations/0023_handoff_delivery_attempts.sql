-- A transport send can have an uncertain outcome. Record its intent before
-- invoking an adapter so a restart cannot silently repeat the side effect.
CREATE TABLE handoff_delivery_attempts (
    id TEXT PRIMARY KEY,
    delivery_id TEXT NOT NULL REFERENCES handoff_delivery_outbox(id),
    sequence INTEGER NOT NULL CHECK (sequence > 0),
    state TEXT NOT NULL CHECK (state IN ('started', 'admitted', 'retryable', 'blocked', 'uncertain')),
    safe_error_code TEXT,
    started_at INTEGER NOT NULL DEFAULT (unixepoch()),
    finished_at INTEGER,
    UNIQUE (delivery_id, sequence)
);
CREATE INDEX handoff_delivery_attempts_unfinished
    ON handoff_delivery_attempts(state, started_at) WHERE state='started';
