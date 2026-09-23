CREATE TABLE agent_connections (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    product TEXT NOT NULL,
    token TEXT NOT NULL,
    publish_enabled INTEGER NOT NULL DEFAULT 0,
    disconnected INTEGER NOT NULL DEFAULT 0,
    last_seen TEXT,
    operation TEXT,
    revision INTEGER NOT NULL DEFAULT 1
);
-- Copy ciphertext unchanged: upgrading must not invalidate the existing connector.
INSERT INTO agent_connections(id,name,product,token,publish_enabled,disconnected,last_seen,operation)
SELECT 'publishing',
    COALESCE((SELECT value FROM publishing_settings WHERE key='agent_connection_name'),'Agent'),
    COALESCE((SELECT value FROM publishing_settings WHERE key='agent_connection_name'),'Agent'),
    value,
    COALESCE((SELECT value!='false' FROM publishing_settings WHERE key='publish_enabled'),1),
    COALESCE((SELECT value='true' FROM publishing_settings WHERE key='agent_disconnected'),0),
    (SELECT last_seen FROM bridge_activity WHERE id=1),
    (SELECT operation FROM bridge_activity WHERE id=1)
FROM publishing_settings WHERE key='agent_token';
DELETE FROM publishing_settings WHERE key IN ('agent_token','agent_connection_name','publish_enabled','agent_disconnected','connection_reset');
ALTER TABLE publications ADD COLUMN agent_id TEXT NOT NULL DEFAULT 'publishing';
ALTER TABLE media ADD COLUMN agent_id TEXT NOT NULL DEFAULT 'publishing';
