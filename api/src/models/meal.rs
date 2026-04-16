use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct MealParseRequest {
    pub text: String,
    pub meal_type: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ParsedItem {
    #[serde(alias = "name", alias = "food", alias = "food_item", alias = "item_name")]
    pub food_name: String,
    pub quantity: f32,
    pub unit: String,
    #[serde(default, alias = "preparation", alias = "prep")]
    pub preparation_method: Option<String>,
    pub confidence: Confidence,
    #[serde(alias = "search_terms", alias = "terms")]
    pub database_search_terms: Vec<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ParsedMeal {
    #[serde(alias = "foods", alias = "food_items", alias = "meal_items")]
    pub items: Vec<ParsedItem>,
}

#[derive(Debug, Serialize)]
pub struct MealNutritionResponse {
    pub sync_identifier: Uuid,
    pub meal_type: String,
    pub items: Vec<MatchedItem>,
    pub totals: Vec<NutrientValue>,
}

#[derive(Debug, Serialize)]
pub struct MatchedItem {
    pub parsed: ParsedItem,
    pub matched_food: Option<MatchedFood>,
    pub grams: Option<f64>,
    pub nutrients: Vec<NutrientValue>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MatchedFood {
    pub fdc_id: i32,
    pub name: String,
    pub data_type: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct NutrientValue {
    pub hk_identifier: String,
    pub unit: String,
    pub amount: f64,
    #[serde(default)]
    pub sparse: bool,
}

/// Compact list-item for the history endpoint. Only the top-line macros are
/// returned so the response size stays bounded; full `final_nutrients` is
/// fetched on demand via `GET /api/meals/{id}`.
#[derive(Debug, Serialize)]
pub struct MealHistorySummary {
    pub id: Uuid,
    pub sync_identifier: Uuid,
    pub raw_text: String,
    pub meal_type: String,
    pub created_at: DateTime<Utc>,
    pub totals: MealHistoryTotals,
}

/// Macro summary computed from `final_nutrients` JSONB at read time. Fields are
/// optional — a meal with no matched foods has empty totals.
#[derive(Debug, Default, Serialize)]
pub struct MealHistoryTotals {
    pub calories_kcal: Option<f64>,
    pub protein_g: Option<f64>,
    pub carbs_g: Option<f64>,
    pub fat_g: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct MealHistoryResponse {
    pub items: Vec<MealHistorySummary>,
    /// Opaque cursor for the next page (the `created_at` of the last returned
    /// row). Clients should pass it back as `?before=...` to fetch older rows.
    pub next_before: Option<DateTime<Utc>>,
}

/// Full meal record returned by the detail endpoint.
#[derive(Debug, Serialize)]
pub struct MealDetail {
    pub id: Uuid,
    pub sync_identifier: Uuid,
    pub raw_text: String,
    pub meal_type: String,
    pub parsed_items: serde_json::Value,
    pub matched_foods: serde_json::Value,
    pub final_nutrients: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
}
