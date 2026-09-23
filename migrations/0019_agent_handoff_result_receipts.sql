-- A result read by the sender is separate from recipient completion.
-- Existing handoffs remain unacknowledged; do not invent historic receipts.
ALTER TABLE agent_handoffs ADD COLUMN result_acknowledged_at TEXT;
