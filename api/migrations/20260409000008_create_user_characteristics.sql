CREATE TABLE user_characteristics (
    user_id UUID PRIMARY KEY REFERENCES users(id),
    biological_sex TEXT,
    date_of_birth DATE,
    blood_type TEXT,
    fitzpatrick_skin_type TEXT,
    wheelchair_use BOOLEAN,
    activity_move_mode TEXT,
    content_hash TEXT NOT NULL DEFAULT '',
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
