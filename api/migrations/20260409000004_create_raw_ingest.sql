CREATE TABLE raw_ingest (
    id BIGSERIAL PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id),
    sync_session_id BIGINT REFERENCES sync_sessions(id),
    endpoint TEXT NOT NULL,
    raw_body JSONB NOT NULL,
    received_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    processed BOOLEAN NOT NULL DEFAULT TRUE,
    error_message TEXT
);

CREATE INDEX idx_raw_ingest_user_time ON raw_ingest(user_id, received_at DESC);
CREATE INDEX idx_raw_ingest_unprocessed ON raw_ingest(processed) WHERE NOT processed;
