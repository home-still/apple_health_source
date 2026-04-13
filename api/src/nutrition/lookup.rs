use sqlx::PgPool;

use crate::error::AppError;
use crate::models::meal::{MatchedFood, NutrientValue};
use crate::nutrition::units::Portion;

/// Try each search term in order; return the first decent match. We prefer
/// full-text rank and fall back to `pg_trgm` similarity for very short inputs.
pub async fn best_match(
    pool: &PgPool,
    search_terms: &[String],
) -> Result<Option<MatchedFood>, AppError> {
    for term in search_terms {
        let trimmed = term.trim();
        if trimmed.is_empty() {
            continue;
        }
        let row: Option<(i32, String, String)> = sqlx::query_as(
            r#"
            SELECT f.fdc_id, f.name, f.data_type
            FROM nutrition.foods f
            WHERE f.search_vector @@ plainto_tsquery('english', $1)
               OR f.name % $1
            ORDER BY
                ts_rank(f.search_vector, plainto_tsquery('english', $1)) DESC,
                similarity(f.name, $1) DESC
            LIMIT 1
            "#,
        )
        .bind(trimmed)
        .fetch_optional(pool)
        .await?;

        if let Some((fdc_id, name, data_type)) = row {
            return Ok(Some(MatchedFood { fdc_id, name, data_type }));
        }
    }
    Ok(None)
}

/// Pull every mapped nutrient for a food as amounts per 100 g.
pub async fn nutrients_per_100g(
    pool: &PgPool,
    fdc_id: i32,
) -> Result<Vec<(String, String, f32, bool)>, AppError> {
    let rows: Vec<(String, String, f32, bool)> = sqlx::query_as(
        r#"
        SELECT m.hk_identifier, n.unit_name, fn.amount, m.sparse
        FROM nutrition.food_nutrients fn
        JOIN nutrition.nutrients n ON n.id = fn.nutrient_id
        JOIN nutrition.nutrient_healthkit_map m ON m.nutrient_id = n.id
        WHERE fn.fdc_id = $1
        "#,
    )
    .bind(fdc_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn portions_for(pool: &PgPool, fdc_id: i32) -> Result<Vec<Portion>, AppError> {
    let rows: Vec<(Option<f32>, Option<String>, Option<String>, f32)> = sqlx::query_as(
        r#"
        SELECT amount, portion_description, modifier, gram_weight
        FROM nutrition.food_portions
        WHERE fdc_id = $1
        "#,
    )
    .bind(fdc_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(amount, portion_description, modifier, gram_weight)| Portion {
            amount,
            portion_description,
            modifier,
            gram_weight,
        })
        .collect())
}

/// Scale the per-100g amounts to the user's actual grams consumed.
pub fn scale_nutrients(per_100g: Vec<(String, String, f32, bool)>, grams: f32) -> Vec<NutrientValue> {
    let factor = grams / 100.0;
    per_100g
        .into_iter()
        .map(|(hk_identifier, unit, amount, sparse)| NutrientValue {
            hk_identifier,
            unit,
            amount: amount * factor,
            sparse,
        })
        .collect()
}
