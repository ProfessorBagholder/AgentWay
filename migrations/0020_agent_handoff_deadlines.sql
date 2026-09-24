-- Rebuild to extend SQLite's status CHECK without changing existing task IDs,
-- rowids (pagination cursors), receipts, or their original unbounded contract.
CREATE TABLE agent_handoffs_new (
    id TEXT PRIMARY KEY,
    request_id TEXT NOT NULL,
    sender_id TEXT NOT NULL REFERENCES agent_connections(id),
    sender_name TEXT NOT NULL,
    recipient_id TEXT NOT NULL REFERENCES agent_connections(id),
    recipient_name TEXT NOT NULL,
    title TEXT NOT NULL,
    instructions TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'queued' CHECK (status IN ('queued','claimed','completed','failed','cancelled','timed_out')),
    result TEXT,
    error TEXT,
    claim_hash TEXT,
    lease_until INTEGER,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    revision INTEGER NOT NULL DEFAULT 1,
    result_acknowledged_at TEXT,
    timeout_seconds INTEGER,
    expires_at INTEGER,
    UNIQUE (sender_id, request_id)
);
INSERT INTO agent_handoffs_new (
    rowid,id,request_id,sender_id,sender_name,recipient_id,recipient_name,title,instructions,
    status,result,error,claim_hash,lease_until,created_at,updated_at,revision,result_acknowledged_at
)
SELECT rowid,id,request_id,sender_id,sender_name,recipient_id,recipient_name,title,instructions,
    status,result,error,claim_hash,lease_until,created_at,updated_at,revision,result_acknowledged_at
FROM agent_handoffs;
DROP TABLE agent_handoffs;
ALTER TABLE agent_handoffs_new RENAME TO agent_handoffs;
CREATE INDEX agent_handoffs_recipient_status ON agent_handoffs(recipient_id, status, created_at);
CREATE INDEX agent_handoffs_sender_created ON agent_handoffs(sender_id, created_at);
CREATE INDEX agent_handoffs_expiry ON agent_handoffs(expires_at)
    WHERE status IN ('queued', 'claimed') AND expires_at IS NOT NULL;
