-- Receiver bindings are separate from connection credentials. Native target and
-- receiver-secret material must be encrypted/hashed by the enrollment service.
CREATE TABLE handoff_receiver_bindings (
    connection_id TEXT PRIMARY KEY REFERENCES agent_connections(id),
    product TEXT NOT NULL,
    target_ciphertext TEXT NOT NULL,
    receiver_secret_hash TEXT NOT NULL,
    generation INTEGER NOT NULL CHECK (generation > 0),
    enabled INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
    verified_at INTEGER,
    proof_expires_at INTEGER,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    CHECK (verified_at IS NULL OR proof_expires_at > verified_at)
);

-- Existing tasks remain pull-only. Automatic tasks pin both binding generations
-- so rotation cannot silently redirect a pending delivery.
ALTER TABLE agent_handoffs ADD COLUMN delivery_mode TEXT NOT NULL DEFAULT 'pull'
    CHECK (delivery_mode IN ('pull', 'automatic'));
ALTER TABLE agent_handoffs ADD COLUMN sender_binding_generation INTEGER;
ALTER TABLE agent_handoffs ADD COLUMN recipient_binding_generation INTEGER;

CREATE TABLE handoff_delivery_outbox (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES agent_handoffs(id),
    kind TEXT NOT NULL CHECK (kind IN ('task_offered', 'result_available')),
    connection_id TEXT NOT NULL REFERENCES agent_connections(id),
    binding_generation INTEGER NOT NULL CHECK (binding_generation > 0),
    state TEXT NOT NULL DEFAULT 'pending'
        CHECK (state IN ('pending', 'admitted', 'delivered', 'blocked', 'expired')),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    due_at INTEGER NOT NULL DEFAULT (unixepoch()),
    expires_at INTEGER NOT NULL,
    native_reference_ciphertext TEXT,
    safe_error_code TEXT,
    admitted_at INTEGER,
    delivered_at INTEGER,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    UNIQUE (task_id, kind)
);
CREATE INDEX handoff_delivery_due ON handoff_delivery_outbox(state, due_at)
    WHERE state IN ('pending', 'blocked');
CREATE INDEX handoff_delivery_connection ON handoff_delivery_outbox(connection_id, state, created_at);
