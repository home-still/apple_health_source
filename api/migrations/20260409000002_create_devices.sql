CREATE TABLE devices (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID NOT NULL REFERENCES users(id),
    identifier_for_vendor TEXT NOT NULL,
    device_name TEXT,
    device_model TEXT,
    system_name TEXT,
    system_version TEXT,
    app_version TEXT,
    watch_model TEXT,
    watch_os_version TEXT,
    first_seen TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_seen TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(user_id, identifier_for_vendor)
);
