-- A product-hosted webhook credential is distinct from the receiver token.
-- Both values are encrypted by the application vault before storage.
CREATE TABLE handoff_grok_webhooks (
    connection_id TEXT PRIMARY KEY REFERENCES agent_connections(id),
    binding_generation INTEGER NOT NULL CHECK (binding_generation > 0),
    url_ciphertext TEXT NOT NULL,
    key_ciphertext TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
