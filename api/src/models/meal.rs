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
    pub food_name: String,
    pub quantity: f32,
    pub unit: String,
    #[serde(default)]
    pub preparation_method: Option<String>,
    pub confidence: Confidence,
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
    pub grams: Option<f32>,
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
    pub amount: f32,
    #[serde(default)]
    pub sparse: bool,
}

#[derive(Debug, Serialize)]
pub struct MealHistoryEntry {
    pub id: Uuid,
    pub sync_identifier: Uuid,
    pub raw_text: String,
    pub meal_type: String,
    pub final_nutrients: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct MealHistoryResponse {
    pub items: Vec<MealHistoryEntry>,
}
