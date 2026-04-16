use std::collections::HashMap;

use axum::Json;
use axum::extract::{Path, Query, State};
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::auth::Claims;
use crate::error::AppError;
use crate::handlers::auth_handler::AppState;
use serde::Deserialize;

use crate::models::meal::{
    MatchedItem, MealDetail, MealHistoryResponse, MealHistorySummary, MealHistoryTotals,
    MealNutritionResponse, MealParseRequest, NutrientValue, ParsedItem,
};
use crate::nutrition::cache::cached_best_match;
use crate::nutrition::lookup::{
    iodine_supplemental, nutrients_per_100g, portions_for, scale_nutrients,
};
use crate::nutrition::units::to_grams;

pub async fn parse_meal(
    State(state): State<std::sync::Arc<AppState>>,
    claims: Claims,
    Json(payload): Json<MealParseRequest>,
) -> Result<Json<MealNutritionResponse>, AppError> {
    let start = std::time::Instant::now();
    let result = parse_meal_inner(state, claims, payload).await;
    let outcome = match &result {
        Ok(_) => "success",
        Err(AppError::BadRequest(_)) => "bad_request",
        Err(_) => "internal",
    };
    metrics::histogram!("meal_parse_latency_seconds", "outcome" => outcome)
        .record(start.elapsed().as_secs_f64());
    result
}

async fn parse_meal_inner(
    state: std::sync::Arc<AppState>,
    claims: Claims,
    payload: MealParseRequest,
) -> Result<Json<MealNutritionResponse>, AppError> {
    tracing::info!(
        user_id = %claims.sub,
        text_len = payload.text.len(),
        meal_type = %payload.meal_type,
        "meal parse start"
    );
    if payload.text.trim().is_empty() {
        tracing::warn!(user_id = %claims.sub, "meal parse rejected: empty text");
        return Err(AppError::BadRequest("text is required".into()));
    }

    let meal_type = payload.meal_type.trim().to_lowercase();
    if !["breakfast", "lunch", "dinner", "snack"].contains(&meal_type.as_str()) {
        tracing::warn!(
            user_id = %claims.sub,
            meal_type = %payload.meal_type,
            "meal parse rejected: invalid meal_type"
        );
        return Err(AppError::BadRequest(format!(
            "invalid meal_type: {:?}",
            payload.meal_type
        )));
    }

    let parsed = state.llm.parse_meal(&payload.text).await?;
    tracing::info!(items = parsed.items.len(), "meal parse llm ok");
    let parsed_items_json = serde_json::to_value(&parsed.items)
        .map_err(|e| AppError::Internal(format!("encode parsed items: {e}")))?;

    let mut matched_items = Vec::with_capacity(parsed.items.len());
    for item in parsed.items {
        matched_items.push(resolve_item(&state, item).await?);
    }

    let totals = aggregate_totals(&matched_items);
    let sync_identifier = Uuid::new_v4();

    let matched_foods_json: Vec<_> = matched_items
        .iter()
        .map(|mi| {
            serde_json::json!({
                "food": mi.matched_food,
                "grams": mi.grams,
            })
        })
        .collect();
    let final_nutrients_json = serde_json::to_value(&totals)
        .map_err(|e| AppError::Internal(format!("encode nutrients: {e}")))?;

    sqlx::query(
        r#"
        INSERT INTO nutrition.meal_logs (
            user_id, sync_identifier, raw_text, meal_type,
            parsed_items, matched_foods, final_nutrients
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
    )
    .bind(claims.sub)
    .bind(sync_identifier)
    .bind(&payload.text)
    .bind(&meal_type)
    .bind(&parsed_items_json)
    .bind(serde_json::Value::Array(matched_foods_json))
    .bind(&final_nutrients_json)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(user_id = %claims.sub, error = ?e, "meal_logs insert failed");
        e
    })?;

    tracing::info!(
        sync_identifier = %sync_identifier,
        totals = totals.len(),
        "meal parse ok"
    );

    let response = MealNutritionResponse {
        sync_identifier,
        meal_type,
        items: matched_items,
        totals,
    };

    Ok(Json(response))
}

async fn resolve_item(state: &AppState, parsed: ParsedItem) -> Result<MatchedItem, AppError> {
    let matched_food = cached_best_match(
        &state.nutrition_cache,
        &state.pool,
        &parsed.database_search_terms,
    )
    .await?;

    let (grams, nutrients) = match &matched_food {
        Some(food) => {
            let portions = portions_for(&state.pool, food.fdc_id).await?;
            let grams = to_grams(parsed.quantity, &parsed.unit, &portions);
            let mut per_100g = nutrients_per_100g(&state.pool, food.fdc_id).await?;

            let has_iodine = per_100g
                .iter()
                .any(|(hk, ..)| hk == "HKQuantityTypeIdentifierDietaryIodine");
            if !has_iodine && let Some(mcg) = iodine_supplemental(&state.pool, &food.name).await? {
                per_100g.push((
                    "HKQuantityTypeIdentifierDietaryIodine".to_string(),
                    "UG".to_string(),
                    mcg,
                    true,
                ));
            }

            let nutrients = match grams {
                Some(g) => scale_nutrients(per_100g, g),
                None => Vec::new(),
            };
            (grams, nutrients)
        }
        None => {
            tracing::info!(
                terms = ?parsed.database_search_terms,
                food_name = %parsed.food_name,
                "no food match"
            );
            (None, Vec::new())
        }
    };

    Ok(MatchedItem {
        parsed,
        matched_food,
        grams,
        nutrients,
    })
}

#[derive(Debug, Deserialize)]
pub struct HistoryParams {
    #[serde(default = "default_limit")]
    pub limit: i64,
    /// Exclusive upper bound on `created_at`. Pair with `limit` to paginate
    /// older rows.
    #[serde(default)]
    pub before: Option<DateTime<Utc>>,
}

fn default_limit() -> i64 {
    50
}

pub async fn history(
    State(state): State<std::sync::Arc<AppState>>,
    claims: Claims,
    Query(params): Query<HistoryParams>,
) -> Result<Json<MealHistoryResponse>, AppError> {
    let limit = params.limit.clamp(1, 200);
    #[allow(clippy::type_complexity)] // sqlx row tuple — type aliasing hurts readability
    let rows: Vec<(Uuid, Uuid, String, String, serde_json::Value, DateTime<Utc>)> = sqlx::query_as(
        r#"
        SELECT id, sync_identifier, raw_text, meal_type, final_nutrients, created_at
        FROM nutrition.meal_logs
        WHERE user_id = $1
          AND ($3::timestamptz IS NULL OR created_at < $3)
        ORDER BY created_at DESC
        LIMIT $2
        "#,
    )
    .bind(claims.sub)
    .bind(limit)
    .bind(params.before)
    .fetch_all(&state.pool)
    .await?;

    let items: Vec<MealHistorySummary> = rows
        .into_iter()
        .map(
            |(id, sync_identifier, raw_text, meal_type, final_nutrients, created_at)| {
                MealHistorySummary {
                    id,
                    sync_identifier,
                    raw_text,
                    meal_type,
                    created_at,
                    totals: summarize_totals(&final_nutrients),
                }
            },
        )
        .collect();

    // Only emit a cursor when we returned a full page — otherwise the caller
    // has already reached the end of their history.
    let next_before = if items.len() as i64 == limit {
        items.last().map(|m| m.created_at)
    } else {
        None
    };

    Ok(Json(MealHistoryResponse { items, next_before }))
}

pub async fn history_detail(
    State(state): State<std::sync::Arc<AppState>>,
    claims: Claims,
    Path(id): Path<Uuid>,
) -> Result<Json<MealDetail>, AppError> {
    #[allow(clippy::type_complexity)] // sqlx row tuple
    let row: Option<(
        Uuid,
        Uuid,
        String,
        String,
        serde_json::Value,
        serde_json::Value,
        serde_json::Value,
        DateTime<Utc>,
        Option<DateTime<Utc>>,
    )> = sqlx::query_as(
        r#"
        SELECT id, sync_identifier, raw_text, meal_type,
               parsed_items, matched_foods, final_nutrients,
               created_at, updated_at
        FROM nutrition.meal_logs
        WHERE id = $1 AND user_id = $2
        "#,
    )
    .bind(id)
    .bind(claims.sub)
    .fetch_optional(&state.pool)
    .await?;

    let row = row.ok_or_else(|| AppError::BadRequest("meal not found".into()))?;
    Ok(Json(MealDetail {
        id: row.0,
        sync_identifier: row.1,
        raw_text: row.2,
        meal_type: row.3,
        parsed_items: row.4,
        matched_foods: row.5,
        final_nutrients: row.6,
        created_at: row.7,
        updated_at: row.8,
    }))
}

/// Extract top-line macros from `final_nutrients` for the history summary view.
/// The JSONB shape is `[{hk_identifier, unit, amount, sparse}, ...]`.
fn summarize_totals(final_nutrients: &serde_json::Value) -> MealHistoryTotals {
    let mut totals = MealHistoryTotals::default();
    let Some(entries) = final_nutrients.as_array() else {
        return totals;
    };
    for entry in entries {
        let (Some(hk), Some(amount)) = (
            entry.get("hk_identifier").and_then(|v| v.as_str()),
            entry.get("amount").and_then(|v| v.as_f64()),
        ) else {
            continue;
        };
        match hk {
            "HKQuantityTypeIdentifierDietaryEnergyConsumed" => totals.calories_kcal = Some(amount),
            "HKQuantityTypeIdentifierDietaryProtein" => totals.protein_g = Some(amount),
            "HKQuantityTypeIdentifierDietaryCarbohydrates" => totals.carbs_g = Some(amount),
            "HKQuantityTypeIdentifierDietaryFatTotal" => totals.fat_g = Some(amount),
            _ => {}
        }
    }
    totals
}

fn aggregate_totals(items: &[MatchedItem]) -> Vec<NutrientValue> {
    let mut acc: HashMap<String, NutrientValue> = HashMap::new();
    for item in items {
        for n in &item.nutrients {
            acc.entry(n.hk_identifier.clone())
                .and_modify(|v| v.amount += n.amount)
                .or_insert_with(|| n.clone());
        }
    }
    let mut out: Vec<NutrientValue> = acc.into_values().collect();
    out.sort_by(|a, b| a.hk_identifier.cmp(&b.hk_identifier));
    out
}
