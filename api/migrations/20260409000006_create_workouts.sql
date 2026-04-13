CREATE TABLE workouts (
    start_date TIMESTAMPTZ NOT NULL,
    user_id UUID NOT NULL REFERENCES users(id),
    hk_uuid TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    activity_type INT NOT NULL,
    activity_name TEXT,
    duration_seconds DOUBLE PRECISION,
    total_energy_burned_kcal DOUBLE PRECISION,
    total_distance_m DOUBLE PRECISION,
    total_swimming_stroke_count DOUBLE PRECISION,
    end_date TIMESTAMPTZ NOT NULL,
    source_device_id UUID,
    metadata JSONB,
    sync_session_id BIGINT,
    synced_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(user_id, hk_uuid, start_date)
);

SELECT create_hypertable('workouts', 'start_date',
    chunk_time_interval => INTERVAL '3 months');

CREATE INDEX idx_workouts_user_date ON workouts(user_id, start_date DESC);
CREATE INDEX idx_workouts_hash_check ON workouts(user_id, hk_uuid, content_hash);
