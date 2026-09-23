CREATE TABLE agent_handoff_grants (
    sender_id TEXT NOT NULL REFERENCES agent_connections(id),
    recipient_id TEXT NOT NULL REFERENCES agent_connections(id),
    PRIMARY KEY (sender_id, recipient_id),
    CHECK (sender_id <> recipient_id)
);

CREATE TABLE agent_handoffs (
    id TEXT PRIMARY KEY,
    request_id TEXT NOT NULL,
    sender_id TEXT NOT NULL REFERENCES agent_connections(id),
    sender_name TEXT NOT NULL,
    recipient_id TEXT NOT NULL REFERENCES agent_connections(id),
    recipient_name TEXT NOT NULL,
    title TEXT NOT NULL,
    instructions TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'queued' CHECK (status IN ('queued','claimed','completed','failed','cancelled')),
    result TEXT,
    error TEXT,
    claim_hash TEXT,
    lease_until INTEGER,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    revision INTEGER NOT NULL DEFAULT 1,
    UNIQUE (sender_id, request_id)
);
CREATE INDEX agent_handoffs_recipient_status ON agent_handoffs(recipient_id, status, created_at);
CREATE INDEX agent_handoffs_sender_created ON agent_handoffs(sender_id, created_at);
CREATE INDEX agent_handoff_events ON events(kind, json_extract(payload,'$.id'), sequence) WHERE kind='agent.handoff';
