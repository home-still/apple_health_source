//! Deterministic quantity-to-grams conversion. Per the doc: unit conversion is
//! done in Rust, never delegated to the LLM.

/// A per-fdc_id portion row (pulled from `nutrition.food_portions`). When a
/// volumetric or arbitrary ("slice", "medium", "piece") unit can't be resolved
/// by the static mass table, we try to match one of these for the food.
#[derive(Debug, Clone)]
pub struct Portion {
    pub amount: Option<f64>,
    pub portion_description: Option<String>,
    pub modifier: Option<String>,
    pub gram_weight: f64,
}

/// Convert a quantity expressed in `unit` into grams. Returns `None` when the
/// unit is not recognized and no matching `Portion` is found.
pub fn to_grams(quantity: f32, unit: &str, portions: &[Portion]) -> Option<f64> {
    let u = normalize_unit(unit);

    // An empty unit matches every portion row via substring (`"".contains(x)`
    // is true), which would silently return nonsensical grams. Reject early.
    if u.is_empty() {
        return None;
    }

    let q = quantity as f64;

    if let Some(grams_per_unit) = mass_grams_per_unit(&u) {
        return Some(q * grams_per_unit);
    }

    if let Some(p) = match_portion(&u, portions) {
        // A portion row with gram_weight <= 0 is garbage input (USDA has a
        // handful of water/air entries like this). Silently scaling by zero
        // would produce an empty nutrient response with no explanation, so
        // treat it the same as "portion unit unresolved".
        if p.gram_weight <= 0.0 {
            return None;
        }
        let per_unit = p.gram_weight / p.amount.filter(|a| *a > 0.0).unwrap_or(1.0);
        return Some(q * per_unit);
    }

    // The LLM defaults to "serving" (per the system prompt) when the user
    // doesn't specify a unit, but USDA's `food_portions` uses concrete terms
    // like "slice", "cup", "piece". Fall back to the smallest positive portion
    // — for pizza that's one slice (117 g) rather than a whole pie (937 g),
    // which matches what people mean by "a serving of pizza".
    if is_generic_serving(&u)
        && let Some(p) = portions
            .iter()
            .filter(|p| p.gram_weight > 0.0)
            .min_by(|a, b| a.gram_weight.partial_cmp(&b.gram_weight).unwrap())
    {
        let per_unit = p.gram_weight / p.amount.filter(|a| *a > 0.0).unwrap_or(1.0);
        return Some(q * per_unit);
    }

    None
}

fn is_generic_serving(unit: &str) -> bool {
    matches!(
        unit,
        "serving" | "servings" | "portion" | "portions" | "piece" | "pieces" | "unit" | "units"
    )
}

fn normalize_unit(unit: &str) -> String {
    unit.trim()
        .to_ascii_lowercase()
        .trim_end_matches('.')
        .to_string()
}

fn mass_grams_per_unit(unit: &str) -> Option<f64> {
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

    #[test]
    fn rejects_empty_unit() {
        let portions = vec![Portion {
            amount: Some(1.0),
            portion_description: Some("slice".into()),
            modifier: Some("slice".into()),
            gram_weight: 28.0,
        }];
        assert_eq!(to_grams(2.0, "", &portions), None);
        assert_eq!(to_grams(2.0, "   ", &portions), None);
    }

    #[test]
    fn rejects_zero_gram_weight_portion() {
        let portions = vec![Portion {
            amount: Some(1.0),
            portion_description: Some("1 unit".into()),
            modifier: Some("unit".into()),
            gram_weight: 0.0,
        }];
        assert_eq!(to_grams(5.0, "unit", &portions), None);
    }

    #[test]
    fn generic_serving_picks_smallest_portion() {
        let portions = vec![
            Portion {
                amount: Some(1.0),
                portion_description: None,
                modifier: Some("pizza".into()),
                gram_weight: 937.0,
            },
            Portion {
                amount: Some(1.0),
                portion_description: None,
                modifier: Some("slice".into()),
                gram_weight: 117.0,
            },
        ];
        assert_eq!(to_grams(1.0, "serving", &portions), Some(117.0));
        assert_eq!(to_grams(2.0, "servings", &portions), Some(234.0));
    }

    #[test]
    fn generic_serving_with_no_portions_returns_none() {
        assert_eq!(to_grams(1.0, "serving", &[]), None);
    }
}
