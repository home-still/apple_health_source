-- Reverts 20260414000005_seed_guards's case-insensitive unique index on
-- nutrition.nutrients(name). USDA FoodData Central legitimately has rows that
-- collide case-insensitively — e.g. "Energy" (kcal, id=1008) and "Energy" (kJ,
-- id=1062), "Oligosaccharides" (G) and "Oligosaccharides" (MG). The original
-- guard assumed USDA had no such collisions; it blocked the real importer.
-- The id-level PRIMARY KEY still protects against true duplicates from a
-- re-seed, which was the original intent.

DROP INDEX IF EXISTS nutrition.nutrients_name_lower;
