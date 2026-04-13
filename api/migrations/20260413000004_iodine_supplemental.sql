-- Supplemental iodine values for foods whose USDA Foundation / SR Legacy rows
-- lack iodine data. Sourced from USDA's "Iodine Content of Common Foods"
-- reference publication; amounts converted to per-100 g for consistency with
-- the primary `food_nutrients` table.

CREATE TABLE nutrition.iodine_supplemental (
    id SERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    iodine_mcg_per_100g REAL NOT NULL,
    search_vector tsvector GENERATED ALWAYS AS (to_tsvector('english', name)) STORED
);
CREATE INDEX idx_iodine_supp_search ON nutrition.iodine_supplemental USING gin(search_vector);
CREATE INDEX idx_iodine_supp_trgm ON nutrition.iodine_supplemental USING gin(name gin_trgm_ops);

INSERT INTO nutrition.iodine_supplemental (name, iodine_mcg_per_100g) VALUES
    ('cod, baked or broiled',           116.5),
    ('yogurt, plain, low-fat',           30.6),
    ('salt, iodized',                  4733.3),
    ('milk, reduced fat',                23.0),
    ('fish sticks',                      63.5),
    ('bread, white, enriched',           90.0),
    ('shrimp, cooked',                   41.2),
    ('ice cream, chocolate',             45.5),
    ('macaroni, enriched, cooked',       19.3),
    ('egg, whole',                       48.0),
    ('tuna, canned in oil',              20.0),
    ('cheese, cheddar',                  42.9),
    ('corn, cream style, canned',        11.5),
    ('prunes, dried',                    32.5),
    ('lima beans, boiled',                9.4),
    ('apple juice, canned',               2.8),
    ('peas, green, frozen, boiled',       4.2),
    ('banana, raw',                       2.5),
    ('raisin bran cereal',               22.0);
