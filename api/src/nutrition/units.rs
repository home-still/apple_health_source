//! Deterministic quantity-to-grams conversion. Per the doc: unit conversion is
//! done in Rust, never delegated to the LLM.

/// A per-fdc_id portion row (pulled from `nutrition.food_portions`). When a
/// volumetric or arbitrary ("slice", "medium", "piece") unit can't be resolved
/// by the static mass table, we try to match one of these for the food.
#[derive(Debug, Clone)]
pub struct Portion {
    pub amount: Option<f32>,
    pub portion_description: Option<String>,
    pub modifier: Option<String>,
    pub gram_weight: f32,
}

/// Convert a quantity expressed in `unit` into grams. Returns `None` when the
/// unit is not recognized and no matching `Portion` is found.
pub fn to_grams(quantity: f32, unit: &str, portions: &[Portion]) -> Option<f32> {
    let u = normalize_unit(unit);

    if let Some(grams_per_unit) = mass_grams_per_unit(&u) {
        return Some(quantity * grams_per_unit);
    }

    if let Some(p) = match_portion(&u, portions) {
        let per_unit = p.gram_weight / p.amount.filter(|a| *a > 0.0).unwrap_or(1.0);
        return Some(quantity * per_unit);
    }

    None
}

fn normalize_unit(unit: &str) -> String {
    unit.trim().to_ascii_lowercase().trim_end_matches('.').to_string()
}

fn mass_grams_per_unit(unit: &str) -> Option<f32> {
    match unit {
        "g" | "gram" | "grams" => Some(1.0),
        "kg" | "kilogram" | "kilograms" => Some(1000.0),
        "mg" | "milligram" | "milligrams" => Some(0.001),
        "oz" | "ounce" | "ounces" => Some(28.3495),
        "lb" | "lbs" | "pound" | "pounds" => Some(453.592),
        // Volume approximations that are safe for water-dense foods; for dense or
        // fluffy foods the `Portion` lookup should win instead.
        "ml" | "milliliter" | "milliliters" => Some(1.0),
        "l" | "liter" | "liters" => Some(1000.0),
        "fl oz" | "floz" | "fluid ounce" | "fluid ounces" => Some(29.5735),
        _ => None,
    }
}

fn match_portion<'a>(unit: &str, portions: &'a [Portion]) -> Option<&'a Portion> {
    portions.iter().find(|p| {
        let desc = p
            .portion_description
            .as_deref()
            .unwrap_or("")
            .to_ascii_lowercase();
        let modi = p.modifier.as_deref().unwrap_or("").to_ascii_lowercase();
        desc.contains(unit) || modi.contains(unit)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mass_units() {
        assert_eq!(to_grams(6.0, "oz", &[]), Some(6.0 * 28.3495));
        assert_eq!(to_grams(2.5, "kg", &[]), Some(2500.0));
        assert_eq!(to_grams(150.0, "g", &[]), Some(150.0));
    }

    #[test]
    fn portion_fallback() {
        let portions = vec![Portion {
            amount: Some(1.0),
            portion_description: Some("1 slice".into()),
            modifier: Some("slice".into()),
            gram_weight: 28.0,
        }];
        assert_eq!(to_grams(2.0, "slice", &portions), Some(56.0));
    }

    #[test]
    fn unknown_returns_none() {
        assert_eq!(to_grams(1.0, "pinch", &[]), None);
    }
}
