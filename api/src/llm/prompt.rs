use serde_json::{Value, json};

pub const SYSTEM_PROMPT: &str = r#"You parse natural-language meal descriptions into structured food items for a nutrition database lookup. You do NOT estimate nutrient values — that is done downstream against the USDA database.

Work step by step (chain-of-thought, internally):
1. Identify every distinct food item the speaker consumed.
2. For each item, extract the quantity, unit, and preparation method if mentioned.
3. Produce an ordered list of progressively simpler database search terms, starting with the specific name and ending with the most generic fallback (e.g., ["grilled chicken breast", "chicken breast", "chicken"]).
4. Compound foods (e.g., "caesar salad", "chicken burrito") should be decomposed into their primary ingredients with estimated proportions, unless they appear in USDA as a composite.
5. Assign confidence: high (explicit quantity + specific food), medium (vague quantity or generic food), low (ambiguous).

Vague-quantity defaults:
- "some" → 0.5 serving
- "a handful" → 1 oz (for nuts/chips/berries)
- "a bowl" → 1.5 cups
- "a glass" → 8 fl oz
- missing → 1 standard USDA serving, confidence = medium

Output strictly valid JSON matching the provided schema. No prose, no markdown, no commentary."#;

pub fn meal_json_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["items"],
        "properties": {
            "items": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": [
                        "food_name",
                        "quantity",
                        "unit",
                        "confidence",
                        "database_search_terms"
                    ],
                    "properties": {
                        "food_name": { "type": "string" },
                        "quantity": { "type": "number", "minimum": 0 },
                        "unit":     { "type": "string" },
                        "preparation_method": { "type": ["string", "null"] },
                        "confidence": { "type": "string", "enum": ["high", "medium", "low"] },
                        "database_search_terms": {
                            "type": "array",
                            "items": { "type": "string" },
                            "minItems": 1
                        }
                    }
                }
            }
        }
    })
}
