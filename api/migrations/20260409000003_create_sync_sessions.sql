CREATE TABLE sync_sessions (
    id BIGSERIAL PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id),
    device_id UUID REFERENCES devices(id),
    sample_type TEXT,
    client_ip TEXT,
    latitude DOUBLE PRECISION,
    longitude DOUBLE PRECISION,
    samples_sent INT NOT NULL DEFAULT 0,
    samples_accepted INT NOT NULL DEFAULT 0,
    samples_skipped INT NOT NULL DEFAULT 0,
    deletions INT NOT NULL DEFAULT 0,
    duration_ms DOUBLE PRECISION,
    status TEXT NOT NULL DEFAULT 'pending',
    error_message TEXT,
    started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMPTZ
);

CREATE INDEX idx_sessions_user_time ON sync_sessions(user_id, started_at DESC);
