CREATE TABLE activity_summaries (
    date DATE NOT NULL,
    user_id UUID NOT NULL REFERENCES users(id),
    content_hash TEXT NOT NULL,
    active_energy_burned DOUBLE PRECISION,
    active_energy_burned_goal DOUBLE PRECISION,
    apple_exercise_time DOUBLE PRECISION,
    apple_exercise_time_goal DOUBLE PRECISION,
    apple_stand_hours INT,
    apple_stand_hours_goal INT,
    apple_move_time INT,
    apple_move_time_goal INT,
    synced_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(user_id, date)
);
