-- Compress health_samples after 30 days
ALTER TABLE health_samples SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'user_id, sample_type',
    timescaledb.compress_orderby = 'start_date DESC'
);
SELECT add_compression_policy('health_samples', INTERVAL '30 days');

-- Compress workouts after 90 days
ALTER TABLE workouts SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'user_id',
    timescaledb.compress_orderby = 'start_date DESC'
);
SELECT add_compression_policy('workouts', INTERVAL '90 days');

-- Compress route points after 30 days
ALTER TABLE workout_route_points SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'user_id',
    timescaledb.compress_orderby = 'timestamp DESC'
);
SELECT add_compression_policy('workout_route_points', INTERVAL '30 days');
