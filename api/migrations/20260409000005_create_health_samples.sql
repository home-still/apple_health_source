CREATE TABLE health_samples (
    start_date TIMESTAMPTZ NOT NULL,
    user_id UUID NOT NULL REFERENCES users(id),
    hk_uuid TEXT NOT NULL,
    sample_type TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    source_name TEXT,
    source_bundle_id TEXT,
    source_device_id UUID,
    end_date TIMESTAMPTZ NOT NULL,
    quantity_value DOUBLE PRECISION,
    quantity_unit TEXT,
    category_value INT,
    correlation_id UUID,
    metadata JSONB,
    sync_session_id BIGINT,
    synced_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(user_id, hk_uuid, start_date)
);

SELECT create_hypertable('health_samples', 'start_date',
    chunk_time_interval => INTERVAL '1 month');

CREATE INDEX idx_samples_user_type_date ON health_samples(user_id, sample_type, start_date DESC);
CREATE INDEX idx_samples_hash_check ON health_samples(user_id, hk_uuid, content_hash);
