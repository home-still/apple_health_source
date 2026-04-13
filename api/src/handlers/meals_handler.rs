use std::collections::HashMap;

use axum::Json;
use axum::extract::State;
use uuid::Uuid;

use crate::auth::Claims;
use crate::error::AppError;
use crate::handlers::auth_handler::AppState;
use crate::models::meal::{
    MatchedItem, MealNutritionResponse, MealParseRequest, NutrientValue, ParsedItem,
};
use crate::nutrition::cache::cached_best_match;
use crate::nutrition::lookup::{nutrients_per_100g, portions_for, scale_nutrients};
use crate::nutrition::units::to_grams;

pub async fn parse_meal(
    State(state): State<std::sync::Arc<AppState>>,
    claims: Claims,
    Json(payload): Json<MealParseRequest>,
) -> Result<Json<MealNutritionResponse>, AppError> {
    if payload.text.trim().is_empty() {
        return Err(AppError::BadRequest("text is required".into()));
    }

    let parsed = state.llm.parse_meal(&payload.text).await?;
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
        INSERT INTO meal_logs (
            user_id, sync_identifier, raw_text, meal_type,
            parsed_items, matched_foods, final_nutrients
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
    )
    .bind(claims.sub)
    .bind(sync_identifier)
    .bind(&payload.text)
    .bind(&payload.meal_type)
    .bind(&parsed_items_json)
    .bind(serde_json::Value::Array(matched_foods_json))
    .bind(&final_nutrients_json)
    .execute(&state.pool)
    .await?;

    let response = MealNutritionResponse {
        sync_identifier,
        meal_type: payload.meal_type,
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
            let per_100g = nutrients_per_100g(&state.pool, food.fdc_id).await?;
            let nutrients = match grams {
                Some(g) => scale_nutrients(per_100g, g),
                None => Vec::new(),
            };
            (grams, nutrients)
        }
        None => (None, Vec::new()),
    };

    Ok(MatchedItem {
        parsed,
        matched_food,
        grams,
        nutrients,
    })
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
