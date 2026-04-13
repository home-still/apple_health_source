CREATE TABLE workout_route_points (
    timestamp TIMESTAMPTZ NOT NULL,
    user_id UUID NOT NULL,
    workout_hk_uuid TEXT NOT NULL,
    latitude DOUBLE PRECISION NOT NULL,
    longitude DOUBLE PRECISION NOT NULL,
    altitude DOUBLE PRECISION,
    horizontal_accuracy DOUBLE PRECISION,
    vertical_accuracy DOUBLE PRECISION,
    speed DOUBLE PRECISION,
    course DOUBLE PRECISION
);

SELECT create_hypertable('workout_route_points', 'timestamp',
    chunk_time_interval => INTERVAL '1 month');

CREATE INDEX idx_routes_workout ON workout_route_points(user_id, workout_hk_uuid, timestamp);
