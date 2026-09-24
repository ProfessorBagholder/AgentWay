-- Owner-triggered, single-send transport probes never confer native readiness.
CREATE TABLE handoff_candidate_probes (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL UNIQUE REFERENCES agent_handoffs(id),
    connection_id TEXT NOT NULL REFERENCES agent_connections(id),
    binding_generation INTEGER NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('started','admitted','not_sent','unknown')),
    started_at INTEGER NOT NULL DEFAULT (unixepoch()),
    finished_at INTEGER,
    UNIQUE (task_id, connection_id, binding_generation)
);
