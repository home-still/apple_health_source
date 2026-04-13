# Apple Health Source — Entity Relationship Diagram

```mermaid
erDiagram
    users {
        uuid id PK "gen_random_uuid()"
        text email UK "NOT NULL"
        text password_hash "Argon2, NOT NULL"
        timestamptz created_at "DEFAULT NOW()"
    }

    devices {
        uuid id PK "gen_random_uuid()"
        uuid user_id FK "NOT NULL"
        text identifier_for_vendor "iOS identifierForVendor"
        text device_name "e.g. Thomas' Phone"
        text device_model "e.g. iPhone17,4"
        text system_name "iOS or watchOS"
        text system_version "e.g. 18.4.1"
        text app_version "e.g. 0.1.0"
        text watch_model "nullable"
        text watch_os_version "nullable"
        timestamptz first_seen "DEFAULT NOW()"
        timestamptz last_seen "DEFAULT NOW()"
    }

    sync_sessions {
        bigint id PK "BIGSERIAL"
        uuid user_id FK "NOT NULL"
        uuid device_id FK "NOT NULL"
        text sample_type "nullable"
        text client_ip "nullable"
        float8 latitude "nullable"
        float8 longitude "nullable"
        int samples_sent "DEFAULT 0"
        int samples_accepted "DEFAULT 0"
        int samples_skipped "hash match"
        int deletions "DEFAULT 0"
        float8 duration_ms "nullable"
        text status "pending|completed|failed"
        text error_message "nullable"
        timestamptz started_at "DEFAULT NOW()"
        timestamptz completed_at "nullable"
    }

    health_samples {
        timestamptz start_date "HYPERTABLE partition key"
        uuid user_id FK "NOT NULL"
        text hk_uuid "HealthKit UUID, NOT NULL"
        text sample_type "NOT NULL"
        text content_hash "SHA-256, NOT NULL"
        text source_name "nullable"
        text source_bundle_id "nullable"
        uuid source_device_id FK "nullable"
        timestamptz end_date "NOT NULL"
        float8 quantity_value "nullable"
        text quantity_unit "nullable"
        int category_value "nullable"
        uuid correlation_id "nullable, BP pairs"
        jsonb metadata "nullable"
        bigint sync_session_id FK "nullable"
        timestamptz synced_at "DEFAULT NOW()"
    }

    workouts {
        timestamptz start_date "HYPERTABLE partition key"
        uuid user_id FK "NOT NULL"
        text hk_uuid "NOT NULL"
        text content_hash "SHA-256, NOT NULL"
        int activity_type "HKWorkoutActivityType"
        text activity_name "nullable"
        float8 duration_seconds "nullable"
        float8 total_energy_burned_kcal "nullable"
        float8 total_distance_m "nullable"
        float8 total_swimming_stroke_count "nullable"
        timestamptz end_date "NOT NULL"
        uuid source_device_id FK "nullable"
        jsonb metadata "nullable"
        bigint sync_session_id FK "nullable"
        timestamptz synced_at "DEFAULT NOW()"
    }

    workout_route_points {
        timestamptz timestamp "HYPERTABLE partition key"
        uuid user_id FK "NOT NULL"
        text workout_hk_uuid "references workout"
        float8 latitude "NOT NULL"
        float8 longitude "NOT NULL"
        float8 altitude "nullable"
        float8 horizontal_accuracy "nullable"
        float8 vertical_accuracy "nullable"
        float8 speed "nullable, m/s"
        float8 course "nullable, degrees"
    }

    activity_summaries {
        date date "NOT NULL"
        uuid user_id FK "NOT NULL"
        text content_hash "SHA-256, NOT NULL"
        float8 active_energy_burned "nullable, kcal"
        float8 active_energy_burned_goal "nullable"
        float8 apple_exercise_time "nullable, min"
        float8 apple_exercise_time_goal "nullable"
        int apple_stand_hours "nullable, 0-24"
        int apple_stand_hours_goal "nullable"
        int apple_move_time "nullable, iOS 17+"
        int apple_move_time_goal "nullable"
        timestamptz synced_at "DEFAULT NOW()"
    }

    user_characteristics {
        uuid user_id PK_FK "users.id"
        text biological_sex "nullable"
        date date_of_birth "nullable"
        text blood_type "nullable"
        text fitzpatrick_skin_type "nullable"
        bool wheelchair_use "nullable"
        text activity_move_mode "nullable"
        text content_hash "SHA-256, NOT NULL"
        timestamptz updated_at "DEFAULT NOW()"
    }

    users ||--o{ devices : "registers"
    users ||--o{ sync_sessions : "initiates"
    users ||--o{ health_samples : "owns"
    users ||--o{ workouts : "owns"
    users ||--o{ activity_summaries : "owns"
    users ||--|| user_characteristics : "has"
    devices ||--o{ sync_sessions : "used in"
    devices ||--o{ health_samples : "source of"
    devices ||--o{ workouts : "source of"
    sync_sessions ||--o{ health_samples : "delivered in"
    sync_sessions ||--o{ workouts : "delivered in"
    workouts ||--o{ workout_route_points : "has route"
```

## Constraints

| Table | Constraint | Columns |
|-------|-----------|---------|
| devices | UNIQUE | (user_id, identifier_for_vendor) |
| health_samples | UNIQUE | (user_id, hk_uuid, start_date) |
| workouts | UNIQUE | (user_id, hk_uuid, start_date) |
| activity_summaries | UNIQUE | (user_id, date) |

## TimescaleDB Configuration

| Table | Chunk Interval | Compress After | Segment By | Order By |
|-------|---------------|----------------|-----------|----------|
| health_samples | 1 month | 30 days | user_id, sample_type | start_date DESC |
| workouts | 3 months | 90 days | user_id | start_date DESC |
| workout_route_points | 1 month | 30 days | user_id | timestamp DESC |

## Indexes

| Table | Index | Columns |
|-------|-------|---------|
| health_samples | idx_samples_user_type_date | (user_id, sample_type, start_date DESC) |
| health_samples | idx_samples_hash_check | (user_id, hk_uuid, content_hash) |
| workouts | idx_workouts_user_date | (user_id, start_date DESC) |
| workouts | idx_workouts_hash_check | (user_id, hk_uuid, content_hash) |
| workout_route_points | idx_routes_workout | (user_id, workout_hk_uuid, timestamp) |
| sync_sessions | idx_sessions_user_time | (user_id, started_at DESC) |

## Notes

- **Hypertable FK limitation:** TimescaleDB does not support foreign keys referencing hypertables. `workout_route_points` references workouts by `workout_hk_uuid` (application-level, not DB-enforced).
- **content_hash:** SHA-256 computed on iOS from canonical payload fields. Used by the `/check` endpoint to skip unchanged data during re-syncs.
- **correlation_id:** Groups related samples (e.g., blood pressure systolic + diastolic measured at the same time). UUID generated on the iOS side.
