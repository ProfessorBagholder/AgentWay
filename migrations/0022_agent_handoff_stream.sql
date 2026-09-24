-- Agent task streams resume from the shared event sequence without scanning
-- unrelated publication and media events.
CREATE INDEX agent_handoff_stream_sequence ON events(sequence)
    WHERE kind = 'agent.handoff';
