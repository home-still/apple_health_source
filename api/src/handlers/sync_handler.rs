use axum::extract::State;
use axum::Json;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use uuid::Uuid;

use crate::auth::Claims;
use crate::error::AppError;
use crate::handlers::auth_handler::AppState;
use crate::models::{
    DeleteRequest, HashCheckRequest, HashCheckResponse, SyncPayload, SyncResponse,
    WorkoutRoutePayload,
};

/// POST /api/v1/health/check
pub async fn hash_check(
    State(state): State<std::sync::Arc<AppState>>,
    claims: Claims,
    Json(payload): Json<HashCheckRequest>,
) -> Result<Json<HashCheckResponse>, AppError> {
    let user_id = claims.sub;

    // Create sync session (skip raw_ingest for /check — payloads are huge and redundant)
    let session_row: (i64,) = sqlx::query_as(
        r#"INSERT INTO sync_sessions (user_id, device_id, sample_type, status)
           VALUES ($1, $2, $3, 'pending')
           RETURNING id"#,
    )
    .bind(user_id)
    .bind(payload.device_id)
    .bind(&payload.sample_type)
    .fetch_one(&state.pool)
    .await?;
    let session_id = session_row.0;

    // Bulk-fetch existing hashes in chunks to avoid Postgres OOM on large arrays
    let mut existing_map: HashMap<String, Option<String>> = HashMap::new();
    let all_uuids: Vec<&str> = payload.items.iter().map(|i| i.hk_uuid.as_str()).collect();

    for chunk in all_uuids.chunks(2000) {
        let chunk_vec: Vec<&str> = chunk.to_vec();
        let rows: Vec<(String, Option<String>)> = sqlx::query_as(
            r#"SELECT hk_uuid, content_hash
               FROM health_samples
               WHERE user_id = $1 AND hk_uuid = ANY($2)"#,
        )
        .bind(user_id)
        .bind(&chunk_vec)
        .fetch_all(&state.pool)
        .await?;

        for (uuid, hash) in rows {
            existing_map.insert(uuid, hash);
        }
    }

    let needed_uuids: Vec<String> = payload
        .items
        .iter()
        .filter(|item| match existing_map.get(&item.hk_uuid) {
            None => true,
            Some(server_hash) => server_hash.as_deref() != Some(item.content_hash.as_str()),
        })
        .map(|item| item.hk_uuid.clone())
        .collect();

    Ok(Json(HashCheckResponse {
        needed_uuids,
        session_id,
    }))
}

/// POST /api/v1/health/sync
///
/// Batch upsert using UNNEST for speed.
pub async fn sync(
    State(state): State<std::sync::Arc<AppState>>,
    claims: Claims,
    Json(payload): Json<SyncPayload>,
) -> Result<Json<SyncResponse>, AppError> {
    let user_id = claims.sub;

    // Record raw ingest
    let raw_body = serde_json::to_value(&payload)
        .map_err(|e| AppError::Internal(format!("serialise: {e}")))?;

    sqlx::query(
        r#"INSERT INTO raw_ingest (user_id, sync_session_id, endpoint, raw_body)
           VALUES ($1, $2, '/api/v1/health/sync', $3)"#,
    )
    .bind(user_id)
    .bind(payload.session_id)
    .bind(&raw_body)
    .execute(&state.pool)
    .await?;

    let mut tx = state.pool.begin().await?;
    let samples_synced = payload.samples.len();
    let workouts_synced = payload.workouts.len();

    // Batch upsert health_samples using UNNEST
    if !payload.samples.is_empty() {
        let mut p_user_ids: Vec<Uuid> = Vec::with_capacity(samples_synced);
        let mut p_hk_uuids: Vec<String> = Vec::with_capacity(samples_synced);
        let mut p_sample_types: Vec<String> = Vec::with_capacity(samples_synced);
        let mut p_content_hashes: Vec<String> = Vec::with_capacity(samples_synced);
        let mut p_source_names: Vec<Option<String>> = Vec::with_capacity(samples_synced);
        let mut p_source_bundle_ids: Vec<Option<String>> = Vec::with_capacity(samples_synced);
        let mut p_source_device_ids: Vec<Option<Uuid>> = Vec::with_capacity(samples_synced);
        let mut p_start_dates: Vec<DateTime<Utc>> = Vec::with_capacity(samples_synced);
        let mut p_end_dates: Vec<DateTime<Utc>> = Vec::with_capacity(samples_synced);
        let mut p_quantity_values: Vec<Option<f64>> = Vec::with_capacity(samples_synced);
        let mut p_quantity_units: Vec<Option<String>> = Vec::with_capacity(samples_synced);
        let mut p_category_values: Vec<Option<i32>> = Vec::with_capacity(samples_synced);
        let mut p_correlation_ids: Vec<Option<Uuid>> = Vec::with_capacity(samples_synced);
        let mut p_metadatas: Vec<Option<serde_json::Value>> = Vec::with_capacity(samples_synced);
        let mut p_session_ids: Vec<i64> = Vec::with_capacity(samples_synced);

        for s in &payload.samples {
            p_user_ids.push(user_id);
            p_hk_uuids.push(s.hk_uuid.clone());
            p_sample_types.push(s.sample_type.clone());
            p_content_hashes.push(s.content_hash.clone().unwrap_or_default());
            p_source_names.push(s.source_name.clone());
            p_source_bundle_ids.push(s.source_bundle_id.clone());
            p_source_device_ids.push(s.source_device_id);
            p_start_dates.push(s.start_date);
            p_end_dates.push(s.end_date);
            p_quantity_values.push(s.quantity_value);
            p_quantity_units.push(s.quantity_unit.clone());
            p_category_values.push(s.category_value);
            p_correlation_ids.push(s.correlation_id);
            p_metadatas.push(s.metadata.clone());
            p_session_ids.push(payload.session_id);
        }

        sqlx::query(
            r#"INSERT INTO health_samples
               (user_id, hk_uuid, sample_type, content_hash,
                source_name, source_bundle_id, source_device_id,
                start_date, end_date,
                quantity_value, quantity_unit, category_value,
                correlation_id, metadata, sync_session_id, synced_at)
               SELECT * FROM UNNEST(
                 $1::uuid[], $2::text[], $3::text[], $4::text[],
                 $5::text[], $6::text[], $7::uuid[],
                 $8::timestamptz[], $9::timestamptz[],
                 $10::float8[], $11::text[], $12::int4[],
                 $13::uuid[], $14::jsonb[], $15::int8[]
               ), NOW()
               ON CONFLICT (user_id, hk_uuid, start_date) DO UPDATE SET
                 content_hash     = EXCLUDED.content_hash,
                 source_name      = EXCLUDED.source_name,
                 source_bundle_id = EXCLUDED.source_bundle_id,
                 source_device_id = EXCLUDED.source_device_id,
                 end_date         = EXCLUDED.end_date,
                 quantity_value   = EXCLUDED.quantity_value,
                 quantity_unit    = EXCLUDED.quantity_unit,
                 category_value   = EXCLUDED.category_value,
                 correlation_id   = EXCLUDED.correlation_id,
                 metadata         = EXCLUDED.metadata,
                 sync_session_id  = EXCLUDED.sync_session_id,
                 synced_at        = NOW()"#,
        )
        .bind(&p_user_ids)
        .bind(&p_hk_uuids)
        .bind(&p_sample_types)
        .bind(&p_content_hashes)
        .bind(&p_source_names)
        .bind(&p_source_bundle_ids)
        .bind(&p_source_device_ids)
        .bind(&p_start_dates)
        .bind(&p_end_dates)
        .bind(&p_quantity_values)
        .bind(&p_quantity_units)
        .bind(&p_category_values)
        .bind(&p_correlation_ids)
        .bind(&p_metadatas)
        .bind(&p_session_ids)
        .execute(&mut *tx)
        .await?;
    }

    // Batch upsert workouts using UNNEST
    if !payload.workouts.is_empty() {
        let n = payload.workouts.len();
        let mut p_user_ids: Vec<Uuid> = Vec::with_capacity(n);
        let mut p_hk_uuids: Vec<String> = Vec::with_capacity(n);
        let mut p_content_hashes: Vec<String> = Vec::with_capacity(n);
        let mut p_activity_types: Vec<i32> = Vec::with_capacity(n);
        let mut p_activity_names: Vec<Option<String>> = Vec::with_capacity(n);
        let mut p_durations: Vec<Option<f64>> = Vec::with_capacity(n);
        let mut p_energy: Vec<Option<f64>> = Vec::with_capacity(n);
        let mut p_distance: Vec<Option<f64>> = Vec::with_capacity(n);
        let mut p_strokes: Vec<Option<i32>> = Vec::with_capacity(n);
        let mut p_start_dates: Vec<DateTime<Utc>> = Vec::with_capacity(n);
        let mut p_end_dates: Vec<DateTime<Utc>> = Vec::with_capacity(n);
        let mut p_device_ids: Vec<Option<Uuid>> = Vec::with_capacity(n);
        let mut p_metadatas: Vec<Option<serde_json::Value>> = Vec::with_capacity(n);
        let mut p_session_ids: Vec<i64> = Vec::with_capacity(n);

        for w in &payload.workouts {
            p_user_ids.push(user_id);
            p_hk_uuids.push(w.hk_uuid.clone());
            p_content_hashes.push(w.content_hash.clone().unwrap_or_default());
            p_activity_types.push(w.activity_type);
            p_activity_names.push(w.activity_name.clone());
            p_durations.push(w.duration_seconds);
            p_energy.push(w.total_energy_burned_kcal);
            p_distance.push(w.total_distance_m);
            p_strokes.push(w.total_swimming_stroke_count);
            p_start_dates.push(w.start_date);
            p_end_dates.push(w.end_date);
            p_device_ids.push(w.source_device_id);
            p_metadatas.push(w.metadata.clone());
            p_session_ids.push(payload.session_id);
        }

        sqlx::query(
            r#"INSERT INTO workouts
               (user_id, hk_uuid, content_hash, activity_type, activity_name,
                duration_seconds, total_energy_burned_kcal, total_distance_m,
                total_swimming_stroke_count,
                start_date, end_date, source_device_id,
                metadata, sync_session_id, synced_at)
               SELECT * FROM UNNEST(
                 $1::uuid[], $2::text[], $3::text[], $4::int4[], $5::text[],
                 $6::float8[], $7::float8[], $8::float8[], $9::int4[],
                 $10::timestamptz[], $11::timestamptz[], $12::uuid[],
                 $13::jsonb[], $14::int8[]
               ), NOW()
               ON CONFLICT (user_id, hk_uuid, start_date) DO UPDATE SET
                 content_hash                = EXCLUDED.content_hash,
                 activity_name               = EXCLUDED.activity_name,
                 duration_seconds            = EXCLUDED.duration_seconds,
                 total_energy_burned_kcal    = EXCLUDED.total_energy_burned_kcal,
                 total_distance_m            = EXCLUDED.total_distance_m,
                 total_swimming_stroke_count = EXCLUDED.total_swimming_stroke_count,
                 end_date                    = EXCLUDED.end_date,
                 source_device_id            = EXCLUDED.source_device_id,
                 metadata                    = EXCLUDED.metadata,
                 sync_session_id             = EXCLUDED.sync_session_id,
                 synced_at                   = NOW()"#,
        )
        .bind(&p_user_ids)
        .bind(&p_hk_uuids)
        .bind(&p_content_hashes)
        .bind(&p_activity_types)
        .bind(&p_activity_names)
        .bind(&p_durations)
        .bind(&p_energy)
        .bind(&p_distance)
        .bind(&p_strokes)
        .bind(&p_start_dates)
        .bind(&p_end_dates)
        .bind(&p_device_ids)
        .bind(&p_metadatas)
        .bind(&p_session_ids)
        .execute(&mut *tx)
        .await?;
    }

    // Handle deletions — constrained by sample_type to leverage the
    // (user_id, sample_type, start_date) index and avoid scanning
    // compressed chunks for unrelated types.
    let mut deleted = 0usize;
    if !payload.deleted_uuids.is_empty() {
        let result = sqlx::query(
            "DELETE FROM health_samples WHERE user_id = $1 AND sample_type = $2 AND hk_uuid = ANY($3)",
        )
        .bind(user_id)
        .bind(&payload.sample_type)
        .bind(&payload.deleted_uuids)
        .execute(&mut *tx)
        .await?;
        deleted = result.rows_affected() as usize;
    }

    // Update sync session
    let latitude = payload.location.as_ref().map(|l| l.latitude);
    let longitude = payload.location.as_ref().map(|l| l.longitude);

    sqlx::query(
        r#"UPDATE sync_sessions
           SET samples_sent = $2, samples_accepted = $2, deletions = $3,
               latitude = $4, longitude = $5, status = 'completed', completed_at = NOW()
           WHERE id = $1"#,
    )
    .bind(payload.session_id)
    .bind((samples_synced + workouts_synced) as i64)
    .bind(deleted as i64)
    .bind(latitude)
    .bind(longitude)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(Json(SyncResponse {
        samples_synced,
        workouts_synced,
        deleted,
    }))
}

/// POST /api/v1/health/delete
pub async fn delete(
    State(state): State<std::sync::Arc<AppState>>,
    claims: Claims,
    Json(payload): Json<DeleteRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let result = sqlx::query(
        "DELETE FROM health_samples WHERE user_id = $1 AND hk_uuid = ANY($2)",
    )
    .bind(claims.sub)
    .bind(&payload.hk_uuids)
    .execute(&state.pool)
    .await?;

    Ok(Json(serde_json::json!({ "deleted": result.rows_affected() })))
}

/// POST /api/v1/health/workout-routes
pub async fn sync_routes(
    State(state): State<std::sync::Arc<AppState>>,
    claims: Claims,
    Json(payload): Json<WorkoutRoutePayload>,
) -> Result<Json<serde_json::Value>, AppError> {
    let user_id = claims.sub;

    // Delete existing points for this workout (full replace)
    sqlx::query(
        "DELETE FROM workout_route_points WHERE user_id = $1 AND workout_hk_uuid = $2",
    )
    .bind(user_id)
    .bind(&payload.workout_hk_uuid)
    .execute(&state.pool)
    .await?;

    if payload.points.is_empty() {
        return Ok(Json(serde_json::json!({ "points_synced": 0 })));
    }

    // Batch INSERT using UNNEST
    let n = payload.points.len();
    let mut p_timestamps: Vec<DateTime<Utc>> = Vec::with_capacity(n);
    let mut p_user_ids: Vec<Uuid> = Vec::with_capacity(n);
    let mut p_workout_uuids: Vec<String> = Vec::with_capacity(n);
    let mut p_latitudes: Vec<f64> = Vec::with_capacity(n);
    let mut p_longitudes: Vec<f64> = Vec::with_capacity(n);
    let mut p_altitudes: Vec<Option<f64>> = Vec::with_capacity(n);
    let mut p_h_acc: Vec<Option<f64>> = Vec::with_capacity(n);
    let mut p_v_acc: Vec<Option<f64>> = Vec::with_capacity(n);
    let mut p_speeds: Vec<Option<f64>> = Vec::with_capacity(n);
    let mut p_courses: Vec<Option<f64>> = Vec::with_capacity(n);

    for p in &payload.points {
        p_timestamps.push(p.timestamp);
        p_user_ids.push(user_id);
        p_workout_uuids.push(payload.workout_hk_uuid.clone());
        p_latitudes.push(p.latitude);
        p_longitudes.push(p.longitude);
        p_altitudes.push(p.altitude);
        p_h_acc.push(p.horizontal_accuracy);
        p_v_acc.push(p.vertical_accuracy);
        p_speeds.push(p.speed);
        p_courses.push(p.course);
    }

    sqlx::query(
        r#"INSERT INTO workout_route_points
           (timestamp, user_id, workout_hk_uuid, latitude, longitude,
            altitude, horizontal_accuracy, vertical_accuracy, speed, course)
           SELECT * FROM UNNEST(
             $1::timestamptz[], $2::uuid[], $3::text[], $4::float8[], $5::float8[],
             $6::float8[], $7::float8[], $8::float8[], $9::float8[], $10::float8[]
           )"#,
    )
    .bind(&p_timestamps)
    .bind(&p_user_ids)
    .bind(&p_workout_uuids)
    .bind(&p_latitudes)
    .bind(&p_longitudes)
    .bind(&p_altitudes)
    .bind(&p_h_acc)
    .bind(&p_v_acc)
    .bind(&p_speeds)
    .bind(&p_courses)
    .execute(&state.pool)
    .await?;

    Ok(Json(serde_json::json!({ "points_synced": n })))
}
