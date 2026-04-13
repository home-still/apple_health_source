CREATE TABLE meal_logs (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    sync_identifier UUID NOT NULL UNIQUE,
    raw_text TEXT NOT NULL,
    meal_type TEXT NOT NULL,
    parsed_items JSONB NOT NULL,
    matched_foods JSONB NOT NULL,
    final_nutrients JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_meal_logs_user_id_created_at ON meal_logs(user_id, created_at DESC);
